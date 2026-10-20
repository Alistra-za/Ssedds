//! NIF texture slots → CS Lighting registers, 1×1 defaults for every reflected SRV, and DDS → SRV in the
//! file's own DXGI format (answers.md Q1: the engine passes the file format through, 0 mismatches over 701
//! slot uses, so no `_SRGB` view is ever forced).
use crate::compile::SrvKind;
use crate::device::Gpu;
use engine::dds::{read_dds, DdsPixelFormat};
use windows::Win32::Graphics::Direct3D::{
    D3D11_SRV_DIMENSION_BUFFER, D3D11_SRV_DIMENSION_TEXTURE2D, D3D11_SRV_DIMENSION_TEXTURE2DARRAY,
    D3D11_SRV_DIMENSION_TEXTURECUBE,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BUFFER_DESC, D3D11_BUFFER_SRV, D3D11_RESOURCE_MISC_BUFFER_STRUCTURED,
    D3D11_RESOURCE_MISC_TEXTURECUBE, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_ARRAY_SRV,
    D3D11_TEX2D_SRV, D3D11_TEXCUBE_SRV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};

/// The Lighting.hlsl register a NIF texture-set slot (0-based) is bound to, or `None` when this
/// technique does not read that slot.
///
/// Table from 04 §2.3 ([Inference] there). Every row the captures exercise is **confirmed** by joining the
/// NIF slots of `000A2C8F.nif` with the bound SRVs of frame11178 (`_probe_…11178\nif_000A2C8F.json` +
/// `state_<ev>.json` resource ids → `texmatch.json` file hits; see the B7 report):
/// - vanilla slot 0 → t0, slot 1 → t1 (every shape);
/// - vanilla slot 2 `_sk` → t12 for FACEGEN (type 4, Arch_Head ev4217) and FACEGEN_RGB_TINT (type 5,
///   Arch_Mouth ev4269);
/// - vanilla slot 7 → t9 on HAIR + BACK_LIGHTING (type 6, Lashes ev5536 `lash_flow.dds`, Brows ev5614);
/// - TruePBR slot 0 → t0, 1 → t1, 3 → t4 (`m_p.dds`), 5 → t5 (`*_rmaos.dds`), 6 → t9 (`m_cnr.dds`),
///   7 → t12 (`teeth_s.dds` ev3924, `m.dds` ev6595).
///
/// Not exercised by any capture (kept from 04): glow slot 2 → t6 (type 2) and PBR emissive slot 2 → t6,
/// parallax slot 3 → t3, cubemap slot 4 → t4, MLP inner slot 6 → t8, MSN slot 7 → t2, FaceGen slot 6 → t3
/// (the engine binds the FaceTint DDS by FormID there, not the NIF path: Arch_Head's slot 6 is
/// `textures\lel` while t3 holds `facetint\…\000a2c8f.dds`).
pub fn register_for_slot(shader_type: u32, pbr: bool, msn: bool, slot: usize) -> Option<u32> {
    if pbr {
        return match slot {
            0 => Some(0),
            1 => Some(1),
            2 => Some(6),
            3 => Some(4),
            5 => Some(5),
            6 => Some(9),
            7 => Some(12),
            _ => None,
        };
    }
    match slot {
        0 => Some(0),
        1 => Some(1),
        2 if shader_type == 2 => Some(6),
        2 => Some(12),
        3 if shader_type == 3 => Some(3),
        3 if shader_type == 4 => Some(4),
        4 if matches!(shader_type, 1 | 11 | 16) => Some(4),
        5 => Some(5),
        6 if shader_type == 11 => Some(8),
        6 if shader_type == 4 => Some(3),
        7 if msn => Some(2),
        7 => Some(9),
        _ => None,
    }
}

/// Default colour of a register with no texture (brief B7): t0 white, t1 flat normal `(128,128,255,255)`,
/// t14 (shadow mask) white = unshadowed, every other register black (t5 env mask, t6 glow …).
pub fn default_for_register(reg: u32) -> [u8; 4] {
    match reg {
        0 | 14 => [255, 255, 255, 255],
        1 => [128, 128, 255, 255],
        _ => [0, 0, 0, 255],
    }
}

/// [`default_for_register`] refined by technique:
/// - TruePBR t5 RMAOS = `(255,0,255,255)`: roughness 1, metal 0, AO 1, specular 1;
/// - FACEGEN (type 4) t3 tint = mid grey and t4 detail = 64 grey: `GetFacegenBaseColor` (Lighting.hlsl ~673)
///   multiplies by `3.984375 × (detail + 1/255)`, so 64/255 ≈ ×1.02 keeps the diffuse, and a 0.5 tint keeps
///   it close to the base (`raw² + raw − raw²/2`); black would make the face black. [Inference: the engine's
///   16×16 RGBA8 default at t4 (ev4217) has unknown content.]
///
/// CS feature registers (P1-C C3; reflected only with CS_SKIN / CS_HAIR): t72 skin detail normal = flat with
/// detail mask 0; t73 hair tangent shift (`Texture2D<float>`, read `.x - 0.5`) = mid grey, no shift; t71 skin
/// extra and t74 skin wetness = 0 (gated off by `skinMaterialFlags` = 0; t71 gets the shape's RFAOS map when
/// [`rfaos_candidates`] finds one).
pub fn default_for(reg: u32, shader_type: u32, pbr: bool) -> [u8; 4] {
    match (reg, pbr, shader_type) {
        (72, _, _) => [128, 128, 255, 0],
        (73, _, _) => [128, 128, 128, 255],
        (71 | 74, _, _) => [0, 0, 0, 0],
        (5, true, _) => [255, 0, 255, 255],
        (3, false, 4) => [128, 128, 128, 255],
        (4, false, 4) => [64, 64, 64, 255],
        _ => default_for_register(reg),
    }
}

/// CS skin extra map (RFAOS, bound at t71) candidates of a FaceGen (4) or FaceGen RGB tint (5) shape, in lookup
/// order (owner rule = pre-October CS `Skin.cpp:451-512`, 02-skin-rfaos.md §0): the canonical slot 7 `…_s.dds`, then
/// slot 1 `…_n.dds` / `…_msn.dds`, each with that suffix replaced by `_rfaos.dds`. The first one the resolver
/// returns wins. Canonical paths are lower case, so the end-anchored match is case-insensitive.
pub fn rfaos_candidates(shader_type: u32, normal: &str, specular: &str) -> Vec<String> {
    if !matches!(shader_type, 4 | 5) {
        return vec![];
    }
    let s = specular.strip_suffix("_s.dds");
    let n = normal
        .strip_suffix("_n.dds")
        .or_else(|| normal.strip_suffix("_msn.dds"));
    [s, n]
        .into_iter()
        .flatten()
        .map(|stem| format!("{stem}_rfaos.dds"))
        .collect()
}

fn srv_from(
    gpu: &Gpu,
    res: &windows::Win32::Graphics::Direct3D11::ID3D11Resource,
    desc: &D3D11_SHADER_RESOURCE_VIEW_DESC,
) -> Result<ID3D11ShaderResourceView, String> {
    let mut v = None;
    // SAFETY: `res` belongs to `gpu.device`; `desc` and the out-pointer are live locals.
    unsafe {
        gpu.device
            .CreateShaderResourceView(res, Some(desc), Some(&mut v))
    }
    .map_err(|e| gpu.fail("CreateShaderResourceView", e))?;
    v.ok_or_else(|| "CreateShaderResourceView returned no view".to_string())
}

fn tex_desc(w: u32, h: u32, mips: u32, array: u32, format: DXGI_FORMAT) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: mips,
        ArraySize: array,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: if array == 6 {
            D3D11_RESOURCE_MISC_TEXTURECUBE.0 as u32
        } else {
            0
        },
    }
}

fn create_texture(
    gpu: &Gpu,
    desc: &D3D11_TEXTURE2D_DESC,
    init: &[D3D11_SUBRESOURCE_DATA],
) -> Result<ID3D11Texture2D, String> {
    let mut t = None;
    // SAFETY: `init` holds `MipLevels × ArraySize` entries whose pointers reference memory that outlives the
    // call (the caller's byte buffers).
    unsafe {
        gpu.device
            .CreateTexture2D(desc, Some(init.as_ptr()), Some(&mut t))
    }
    .map_err(|e| gpu.fail("CreateTexture2D", e))?;
    t.ok_or_else(|| "CreateTexture2D returned no texture".to_string())
}

fn view_2d(format: DXGI_FORMAT, mips: u32) -> D3D11_SHADER_RESOURCE_VIEW_DESC {
    D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: mips,
            },
        },
    }
}

fn view_cube(format: DXGI_FORMAT, mips: u32) -> D3D11_SHADER_RESOURCE_VIEW_DESC {
    D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURECUBE,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            TextureCube: D3D11_TEXCUBE_SRV {
                MostDetailedMip: 0,
                MipLevels: mips,
            },
        },
    }
}

/// A resource of the right dimension for a reflected SRV of `kind`: a 1×1 2D texture, a 1×1 single-slice
/// 2D array, a 2×2×6 cube (every texel `rgba`), or a 1-element zeroed (structured) buffer.
pub fn default_srv(
    gpu: &Gpu,
    kind: SrvKind,
    rgba: [u8; 4],
) -> Result<ID3D11ShaderResourceView, String> {
    let fmt = DXGI_FORMAT_R8G8B8A8_UNORM;
    let px = rgba;
    let one = D3D11_SUBRESOURCE_DATA {
        pSysMem: px.as_ptr() as _,
        SysMemPitch: 4,
        SysMemSlicePitch: 4,
    };
    match kind {
        SrvKind::Tex2D => {
            let t = create_texture(gpu, &tex_desc(1, 1, 1, 1, fmt), &[one])?;
            srv_from(gpu, &t.into(), &view_2d(fmt, 1))
        }
        SrvKind::Tex2DArray => {
            let t = create_texture(gpu, &tex_desc(1, 1, 1, 1, fmt), &[one])?;
            let d = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: fmt,
                ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                        MostDetailedMip: 0,
                        MipLevels: 1,
                        FirstArraySlice: 0,
                        ArraySize: 1,
                    },
                },
            };
            srv_from(gpu, &t.into(), &d)
        }
        SrvKind::Cube => {
            // 2×2, not 1×1: under DYNAMIC_CUBEMAPS a 1×1 env cube black at mip 15 is CS's dynamic-cubemap marker
            // (Lighting.hlsl:2024-2046), and a missing cube must mean "no reflection" (D5 R2).
            let px4 = [rgba; 4];
            let face = D3D11_SUBRESOURCE_DATA {
                pSysMem: px4.as_ptr() as _,
                SysMemPitch: 8,
                SysMemSlicePitch: 16,
            };
            let t = create_texture(gpu, &tex_desc(2, 2, 1, 6, fmt), &[face; 6])?;
            srv_from(gpu, &t.into(), &view_cube(fmt, 1))
        }
        SrvKind::Buffer | SrvKind::Structured => {
            let structured = kind == SrvKind::Structured;
            let zero = [0u8; 16];
            let bd = D3D11_BUFFER_DESC {
                ByteWidth: 16,
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: if structured {
                    D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32
                } else {
                    0
                },
                StructureByteStride: if structured { 16 } else { 0 },
            };
            let init = D3D11_SUBRESOURCE_DATA {
                pSysMem: zero.as_ptr() as _,
                SysMemPitch: 16,
                SysMemSlicePitch: 16,
            };
            let mut b: Option<ID3D11Buffer> = None;
            // SAFETY: descriptors and `zero` are live locals for the call.
            unsafe { gpu.device.CreateBuffer(&bd, Some(&init), Some(&mut b)) }
                .map_err(|e| gpu.fail("CreateBuffer", e))?;
            let b = b.ok_or("CreateBuffer returned no buffer")?;
            let mut el = D3D11_BUFFER_SRV::default();
            el.Anonymous2.NumElements = 1;
            let d = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: if structured {
                    DXGI_FORMAT_UNKNOWN
                } else {
                    DXGI_FORMAT_R32G32B32A32_FLOAT
                },
                ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Buffer: el },
            };
            srv_from(gpu, &b.into(), &d)
        }
    }
}

/// Largest cube side accepted (D3D11's maximum cube dimension).
pub const MAX_CUBE: u32 = 16384;

/// Bytes of one mip of an `size`² cube face, `bpp` bytes per texel, in checked u64.
fn level_bytes(size: u32, m: u32, bpp: u32) -> Option<u64> {
    let w = u64::from((size >> m).max(1));
    w.checked_mul(w)?.checked_mul(u64::from(bpp))
}

/// Bytes of the 6 faces of a `size`² cube with `mips` levels: Err unless 1 ≤ size ≤ [`MAX_CUBE`], 1 ≤ mips ≤ the full
/// chain, and the u64 arithmetic does not overflow (hostile headers, review I1).
fn cube_bytes(size: u32, mips: u32, bpp: u32) -> Result<u64, String> {
    if size == 0 || size > MAX_CUBE || mips == 0 || mips > 32 - size.leading_zeros() {
        return Err(format!("cube {size}² with {mips} mips is not uploadable"));
    }
    let face = (0..mips).try_fold(0u64, |t, m| t.checked_add(level_bytes(size, m, bpp)?));
    face.and_then(|f| f.checked_mul(6))
        .ok_or_else(|| format!("cube {size}² × {mips} mips: size overflows"))
}

/// Immutable cube SRV from tight subresources in D3D order (face-major, mip-minor), `bpp` bytes per texel (an
/// uncompressed format). `data` must be exactly the 6 faces of `mips` levels.
pub fn cube_srv(
    gpu: &Gpu,
    size: u32,
    mips: u32,
    format: DXGI_FORMAT,
    bpp: u32,
    data: &[u8],
) -> Result<ID3D11ShaderResourceView, String> {
    let need = cube_bytes(size, mips, bpp)?;
    if data.len() as u64 != need {
        return Err(format!(
            "cube {size}² × {mips} mips: {} bytes, need {need}",
            data.len()
        ));
    }
    let mut init = Vec::with_capacity(6 * mips as usize);
    let mut at = 0usize;
    for _ in 0..6 {
        for m in 0..mips {
            let w = (size >> m).max(1);
            // Bounded by `need` = data.len(), which already fits in memory; the pitches must also fit D3D's u32.
            let slice = level_bytes(size, m, bpp).unwrap_or(u64::MAX);
            let (pitch, slice) = (
                w.checked_mul(bpp).ok_or("cube row pitch overflows")?,
                u32::try_from(slice).map_err(|_| "cube slice over 4 GiB")?,
            );
            init.push(D3D11_SUBRESOURCE_DATA {
                pSysMem: data[at..].as_ptr() as _,
                SysMemPitch: pitch,
                SysMemSlicePitch: slice,
            });
            at += slice as usize;
        }
    }
    let t = create_texture(gpu, &tex_desc(size, size, mips, 6, format), &init)?;
    srv_from(gpu, &t.into(), &view_cube(format, mips))
}

/// Bytes per texel of the cube formats [`read_cube_dds`] accepts: R11G11B10_FLOAT (26), RGBA16F (10), RGBA32F (2).
pub fn cube_bpp(format: DXGI_FORMAT) -> Option<u32> {
    match format.0 {
        26 => Some(4),
        10 => Some(8),
        2 => Some(16),
        _ => None,
    }
}

/// A DX10 cube DDS as RenderDoc saves it: (size, mips, format, data). Accepts only R11G11B10_FLOAT (26), RGBA16F (10)
/// and RGBA32F (2), square, 6 faces (cube flag with arraySize 1, or arraySize 6 without it), data exactly the faces.
/// Anything else is Err.
pub fn read_cube_dds(bytes: &[u8]) -> Result<(u32, u32, DXGI_FORMAT, &[u8]), String> {
    let u = |o: usize| {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or("DDS shorter than its DX10 header")
    };
    if bytes.get(..4) != Some(b"DDS ") || u(4)? != 124 || bytes.get(84..88) != Some(b"DX10") {
        return Err("not a DX10 DDS".into());
    }
    let (h, w) = (u(12)?, u(16)?);
    let mips = if u(8)? & 0x20000 != 0 {
        u(28)?.max(1)
    } else {
        1
    };
    let (format, dim, misc, array) = (DXGI_FORMAT(u(128)? as i32), u(132)?, u(136)?, u(140)?);
    let bpp =
        cube_bpp(format).ok_or_else(|| format!("DDS format {} is not a cube format", format.0))?;
    if dim != 3 || w != h {
        return Err(format!(
            "DDS {w}x{h}, dimension {dim}: not a square 2D cube"
        ));
    }
    let need = cube_bytes(w, mips, bpp).map_err(|e| format!("DDS {e}"))?;
    let cube = misc & D3D11_RESOURCE_MISC_TEXTURECUBE.0 as u32 != 0;
    if !(cube && array == 1 || !cube && array == 6) {
        return Err(format!(
            "DDS is not 6 faces (misc {misc:#x}, arraySize {array})"
        ));
    }
    let data = bytes.get(148..).ok_or("DDS shorter than its DX10 header")?;
    if data.len() as u64 != need {
        return Err(format!("DDS cube data {} bytes, need {need}", data.len()));
    }
    Ok((w, mips, format, data))
}

/// The DXGI format a DDS is uploaded in: the DX10 header's own value, else the legacy header's only
/// possible format (FourCC DXT1/DXT5/BC4U/BC5U and the uncompressed masks are always UNORM, answers.md Q1.2).
pub fn dxgi_format(f: DdsPixelFormat, dx10: Option<u32>) -> DXGI_FORMAT {
    if let Some(d) = dx10 {
        return DXGI_FORMAT(d as i32);
    }
    use DdsPixelFormat as P;
    DXGI_FORMAT(match f {
        P::Bc1 { srgb } => 71 + srgb as i32,
        P::Bc2 { srgb } => 74 + srgb as i32,
        P::Bc3 { srgb } => 77 + srgb as i32,
        P::Bc4 => 80,
        P::Bc5 => 83,
        P::Bc6hUf16 => 95,
        P::Bc6hSf16 => 96,
        P::Bc7 { srgb } => 98 + srgb as i32,
        P::Rgba8 { srgb } => 28 + srgb as i32,
        P::Bgra8 { srgb } => 87 + 4 * srgb as i32,
        P::Bgrx8 => 88,
        P::R8 => 61,
        P::Rg8 => 49,
        P::Rgba16f => 10,
        P::Rgba32f => 2,
    })
}

/// The UNORM twin of an `_SRGB` format (R8G8B8A8, BC1-3, B8G8R8A8, B8G8R8X8, BC7); any other format as is.
pub fn without_srgb(f: DXGI_FORMAT) -> DXGI_FORMAT {
    DXGI_FORMAT(match f.0 {
        29 => 28,
        72 => 71,
        75 => 74,
        78 => 77,
        91 => 87,
        93 => 88,
        99 => 98,
        x => x,
    })
}

/// A decoded DDS ready for binding.
#[derive(Clone)]
pub struct Texture {
    pub srv: ID3D11ShaderResourceView,
    pub kind: SrvKind,
    pub format: DXGI_FORMAT,
}

/// The kind of a bound view, from its dimension. A `ShapeDraw` keeps no reflection, and `bind_textures` only binds a
/// view whose kind equals the reflected one (defaults are made by reflected kind), so this is the kind the register
/// reads; the hot-swap checks a new texture against it.
pub fn view_kind(v: &ID3D11ShaderResourceView) -> Option<SrvKind> {
    let mut d = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: `d` is a live local the call fills in; `v` is a live view.
    unsafe { v.GetDesc(&mut d) };
    match d.ViewDimension {
        x if x == D3D11_SRV_DIMENSION_TEXTURE2D => Some(SrvKind::Tex2D),
        x if x == D3D11_SRV_DIMENSION_TEXTURE2DARRAY => Some(SrvKind::Tex2DArray),
        x if x == D3D11_SRV_DIMENSION_TEXTURECUBE => Some(SrvKind::Cube),
        _ => None,
    }
}

/// DDS bytes → immutable texture + SRV in the file's own format (2D, or a cube when the header says so).
/// Arrays upload slice 0 only. Errors (unsupported header, BC top level not a multiple of 4, …) are text.
pub fn load_dds(gpu: &Gpu, bytes: &[u8]) -> Result<Texture, String> {
    load_dds_as(gpu, bytes, false)
}

/// As [`load_dds`]; `linear` uploads an `_SRGB` file as its UNORM twin (the same bytes, no sRGB decode on
/// sampling), for data maps such as the skin RFAOS at t71.
pub fn load_dds_as(gpu: &Gpu, bytes: &[u8], linear: bool) -> Result<Texture, String> {
    let f = read_dds(bytes).map_err(|e| format!("{e:?}"))?;
    let format = dxgi_format(f.format, f.dx10.map(|d| d.dxgi));
    let format = if linear { without_srgb(format) } else { format };
    let mips = f.levels.len().max(1);
    let slice_bytes = f.levels.last().map_or(0, |l| l.2.end);
    let faces = if f.is_cubemap { 6 } else { 1 };
    let row = |w: u32| match f.format.block_bytes() {
        Some(b) => w.div_ceil(4) as usize * b,
        None => f.format.level_size(w, 1),
    };
    let mut init = Vec::with_capacity(mips * faces);
    for face in 0..faces {
        for (w, h, r) in &f.levels {
            let start = face * slice_bytes + r.start;
            let data = f
                .data
                .get(start..start + r.len())
                .ok_or("DDS data shorter than its levels")?;
            init.push(D3D11_SUBRESOURCE_DATA {
                pSysMem: data.as_ptr() as _,
                SysMemPitch: row(*w) as u32,
                SysMemSlicePitch: f.format.level_size(*w, *h) as u32,
            });
        }
    }
    let desc = tex_desc(f.width, f.height, mips as u32, faces as u32, format);
    let t = create_texture(gpu, &desc, &init)?;
    let (view, kind) = if f.is_cubemap {
        (view_cube(format, mips as u32), SrvKind::Cube)
    } else {
        (view_2d(format, mips as u32), SrvKind::Tex2D)
    };
    Ok(Texture {
        srv: srv_from(gpu, &t.into(), &view)?,
        kind,
        format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_headers_are_unorm_dx10_passes_through() {
        assert_eq!(dxgi_format(DdsPixelFormat::Bc1 { srgb: false }, None).0, 71);
        assert_eq!(dxgi_format(DdsPixelFormat::Bc3 { srgb: false }, None).0, 77);
        assert_eq!(dxgi_format(DdsPixelFormat::Bc4, None).0, 80);
        assert_eq!(
            dxgi_format(DdsPixelFormat::Bc7 { srgb: true }, Some(99)).0,
            99
        );
        assert_eq!(dxgi_format(DdsPixelFormat::Bc7 { srgb: true }, None).0, 99);
        assert_eq!(
            dxgi_format(DdsPixelFormat::Bgra8 { srgb: false }, None).0,
            87
        );
    }

    fn norm(raw: &str) -> String {
        nif::path::canon_texture(raw.as_bytes()).norm
    }

    #[test]
    fn rfaos_candidates_follow_the_old_cs_order() {
        let (n, s) = (
            norm(r"textures\arch\skin\head_N.DDS"),
            norm(r"textures\arch\skin\light\head_s.dds"),
        );
        assert_eq!(
            rfaos_candidates(4, &n, &s),
            [
                "textures/arch/skin/light/head_rfaos.dds",
                "textures/arch/skin/head_rfaos.dds"
            ]
        );
        assert_eq!(
            rfaos_candidates(5, &norm(r"arch\skin\body_msn.dds"), ""),
            ["textures/arch/skin/body_rfaos.dds"]
        );
        // no _s / _n / _msn suffix (anchored at the end): no candidate
        assert!(rfaos_candidates(4, "textures/a/head.dds", "textures/a/head_s_x.dds").is_empty());
        // not a skin technique
        assert!(rfaos_candidates(0, &n, &s).is_empty());
        assert!(rfaos_candidates(6, &n, &s).is_empty());
    }

    #[test]
    fn technique_defaults() {
        assert_eq!(default_for(5, 0, true), [255, 0, 255, 255]);
        assert_eq!(default_for(5, 0, false), [0, 0, 0, 255]);
        assert_eq!(default_for(3, 4, false), [128, 128, 128, 255]);
        assert_eq!(default_for(14, 0, false), [255, 255, 255, 255]);
        // P1-C C3: CS_SKIN / CS_HAIR registers get neutral 1x1 defaults, whatever the technique.
        assert_eq!(default_for(72, 0, false), [128, 128, 255, 0]); // skin detail normal: flat, mask 0
        assert_eq!(default_for(73, 6, false), [128, 128, 128, 255]); // hair tangent shift: .x - 0.5 ~ 0
        assert_eq!(default_for(71, 0, false), [0, 0, 0, 0]); // skin extra (gated off by skinMaterialFlags)
        assert_eq!(default_for(74, 0, true), [0, 0, 0, 0]); // skin wetness (gated off)
    }
}
