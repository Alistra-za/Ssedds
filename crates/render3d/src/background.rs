//! The studio HDRI as a background: one full-screen triangle at the far plane that samples the equirectangular
//! image by the world direction of each pixel. Repo-owned shader (not CS's), compiled with `D3DCompile` once.
//! The image is uploaded once as RGBA16F; the pass does not touch the depth buffer.
use crate::device::Gpu;
use crate::hdri::{self, YAW};
use crate::scene::{inverse4, View};
use std::ffi::CString;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_SRV_DIMENSION_TEXTURE2D};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11DepthStencilState, ID3D11PixelShader, ID3D11SamplerState,
    ID3D11ShaderResourceView, ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER,
    D3D11_DEPTH_STENCIL_DESC, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_FLOAT32_MAX,
    D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0,
    D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_TEXTURE_ADDRESS_WRAP, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};

const SOURCE: &str = r#"
cbuffer Bg : register(b0) { row_major float4x4 InvViewProj; float4 Flags; };
Texture2D<float4> Env : register(t0);
SamplerState Smp : register(s0);
struct VsOut { float4 pos : SV_Position; float2 ndc : TEXCOORD0; };

VsOut vs(uint id : SV_VertexID)
{
    float2 p = float2((id << 1) & 2, id & 2) * 2.0 - 1.0;
    VsOut o;
    o.pos = float4(p, 1.0, 1.0);
    o.ndc = p;
    return o;
}

float4 ps(VsOut i) : SV_Target
{
    // The view-projection is camera-relative (no translation), so the far-plane point is the world direction.
    float4 h = mul(InvViewProj, float4(i.ndc, 1.0, 1.0));
    float3 d = normalize(h.xyz / h.w);
    float u = frac((atan2(d.y, d.x) + YAW - Flags.y) / 6.2831853 + 0.5);
    float v = acos(clamp(d.z, -1.0, 1.0)) / 3.1415927;
    float3 c = Env.SampleLevel(Smp, float2(u, v), 0).rgb;
    // Flags.x: the readback does not sRGB-encode, so give it display values.
    if (Flags.x > 0.5)
        c = c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    return float4(c, 1.0);
}
"#;

fn compile(entry: &str, target: &str) -> Result<Vec<u8>, String> {
    // `YAW` is the one definition shared with the CPU side (`hdri::dir_to_uv`).
    let src = format!("#define YAW ({YAW:.6})\n{SOURCE}");
    let (e, t) = (CString::new(entry).unwrap(), CString::new(target).unwrap());
    let (mut code, mut errs): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: source and names outlive the call; out-pointers are locals.
    let r = unsafe {
        D3DCompile(
            src.as_ptr() as _,
            src.len(),
            PCSTR(c"background.hlsl".as_ptr() as _),
            None,
            None,
            PCSTR(e.as_ptr() as _),
            PCSTR(t.as_ptr() as _),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    let blob = |b: &ID3DBlob| {
        // SAFETY: a blob's pointer/size describe a live buffer.
        unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
            .to_vec()
    };
    match r {
        Ok(()) => Ok(blob(&code.ok_or("D3DCompile returned no bytecode")?)),
        Err(x) => Err(errs
            .map(|b| String::from_utf8_lossy(&blob(&b)).to_string())
            .unwrap_or_else(|| x.to_string())),
    }
}

/// The compiled pass and the uploaded image.
pub struct Backdrop {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cb: ID3D11Buffer,
    env: ID3D11ShaderResourceView,
    no_depth: ID3D11DepthStencilState,
    /// Wrap in U (azimuth), clamp in V (the poles must not bleed into each other).
    sampler: ID3D11SamplerState,
}

impl Backdrop {
    /// Decodes the HDRI (first use) and creates the shaders, the image texture and the state.
    pub fn new(gpu: &Gpu) -> Result<Backdrop, String> {
        let img = &hdri::studio().image;
        let half: Vec<u16> = img
            .rgb
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 1.0])
            .map(|v| half::f16::from_f32(v.min(65000.0)).to_bits())
            .collect();
        let td = D3D11_TEXTURE2D_DESC {
            Width: img.width as u32,
            Height: img.height as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: half.as_ptr() as _,
            SysMemPitch: img.width as u32 * 8,
            SysMemSlicePitch: 0,
        };
        let vs_code = compile("vs", "vs_5_0")?;
        let ps_code = compile("ps", "ps_5_0")?;
        let bd = D3D11_BUFFER_DESC {
            ByteWidth: 80,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let dd = D3D11_DEPTH_STENCIL_DESC::default(); // DepthEnable = false
        let smd = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        let (mut vs, mut ps, mut cb, mut tex, mut env, mut ds, mut smp) =
            (None, None, None, None, None, None, None);
        // SAFETY: descriptors, `half` and the bytecode outlive the calls; out-pointers are locals.
        unsafe {
            gpu.device
                .CreateVertexShader(&vs_code, None, Some(&mut vs))
                .map_err(|e| gpu.fail("CreateVertexShader(background)", e))?;
            gpu.device
                .CreatePixelShader(&ps_code, None, Some(&mut ps))
                .map_err(|e| gpu.fail("CreatePixelShader(background)", e))?;
            gpu.device
                .CreateBuffer(&bd, None, Some(&mut cb))
                .map_err(|e| gpu.fail("CreateBuffer(background)", e))?;
            gpu.device
                .CreateTexture2D(&td, Some(&data), Some(&mut tex))
                .map_err(|e| gpu.fail("CreateTexture2D(HDRI)", e))?;
            let tex = tex.ok_or("no HDRI texture")?;
            gpu.device
                .CreateShaderResourceView(&tex, Some(&sd), Some(&mut env))
                .map_err(|e| gpu.fail("CreateShaderResourceView(HDRI)", e))?;
            gpu.device
                .CreateSamplerState(&smd, Some(&mut smp))
                .map_err(|e| gpu.fail("CreateSamplerState(background)", e))?;
            gpu.device
                .CreateDepthStencilState(&dd, Some(&mut ds))
                .map_err(|e| gpu.fail("CreateDepthStencilState", e))?;
        }
        Ok(Backdrop {
            vs: vs.ok_or("no vertex shader")?,
            ps: ps.ok_or("no pixel shader")?,
            cb: cb.ok_or("no buffer")?,
            env: env.ok_or("no HDRI view")?,
            no_depth: ds.ok_or("no depth state")?,
            sampler: smp.ok_or("no sampler")?,
        })
    }

    /// Draws the background into the bound target (viewport and render target already set), the environment turned by
    /// `yaw` radians about +Z (`Flags.y`). Leaves the default depth state and no input layout.
    pub fn draw(&self, gpu: &Gpu, view: &View, encode: bool, yaw: f32) {
        let flags = [if encode { 1.0f32 } else { 0.0 }, yaw, 0.0, 0.0];
        let inv: Vec<u8> = inverse4(&view.view_proj)
            .iter()
            .flatten()
            .chain(flags.iter())
            .flat_map(|v| v.to_le_bytes())
            .collect();
        // SAFETY: every object belongs to `gpu.device`; `inv` holds the 64 bytes of the buffer.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.UpdateSubresource(&self.cb, 0, None, inv.as_ptr() as _, 0, 0);
            ctx.OMSetDepthStencilState(&self.no_depth, 0);
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            ctx.PSSetShaderResources(0, Some(&[Some(self.env.clone())]));
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
            ctx.Draw(3, 0);
            ctx.OMSetDepthStencilState(None, 0);
        }
    }
}
