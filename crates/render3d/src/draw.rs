//! One NIF shape → one forward CS Lighting draw: permutation, VS/PS, zero-initialised cbuffers filled from
//! the NIF material and the stub scene, SRVs for every reflected register, one sampler per reflected slot.
//! P0 is forward only (no `DEFERRED`, no 8 MRTs); in-game opaque objects take the deferred path
//! (answers.md, hint 15), but the forward permutation of the same Lighting.hlsl is what P0 renders.
use crate::cbuffer::CbWriter;
use crate::compile::{Compiled, Reflection, SrvKind};
use crate::device::Gpu;
use crate::material::{write_material, Bound};
use crate::mesh::GpuMesh;
use crate::scene::{Mat3x4, Scene, View};
use nif::material::MaterialState;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_USAGE_DEFAULT,
};

/// A cbuffer of one stage: its slot, the writer and the GPU buffer it is copied into each frame.
pub struct Cb {
    pub slot: u32,
    pub writer: CbWriter,
    pub buffer: ID3D11Buffer,
}

/// One zero-initialised [`Cb`] per reflected cbuffer.
pub fn cbuffers(gpu: &Gpu, r: &Reflection) -> Result<Vec<Cb>, String> {
    r.cbuffers
        .iter()
        .map(|l| {
            let writer = CbWriter::new(l);
            let d = D3D11_BUFFER_DESC {
                ByteWidth: writer.bytes().len().max(16) as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                StructureByteStride: 0,
            };
            let mut b = None;
            // SAFETY: descriptor is a local; no initial data.
            unsafe { gpu.device.CreateBuffer(&d, None, Some(&mut b)) }
                .map_err(|e| gpu.fail(&format!("CreateBuffer({})", l.name), e))?;
            Ok(Cb {
                slot: l.slot,
                writer,
                buffer: b.ok_or("CreateBuffer returned no buffer")?,
            })
        })
        .collect()
}

/// Everything needed to draw one shape.
pub struct ShapeDraw {
    pub name: String,
    pub vs: ID3D11VertexShader,
    pub ps: ID3D11PixelShader,
    pub mesh: GpuMesh,
    pub vs_cbs: Vec<Cb>,
    pub ps_cbs: Vec<Cb>,
    /// (register, view) for every SRV the PS reflects.
    pub srvs: Vec<(u32, ID3D11ShaderResourceView)>,
    pub samplers: Vec<u32>,
    pub model: Mat3x4,
    pub blend: bool,
    pub skinned: bool,
    /// Registers bound to a real texture (not a 1×1 default); the PBR flags and `EnvmapData.y` read it.
    pub bound: Bound,
}

/// The VS/PS pair of a permutation, compiled and created once per renderer.
#[derive(Clone)]
pub struct Program {
    pub vs: Compiled,
    pub ps: Compiled,
    pub vs_obj: ID3D11VertexShader,
    pub ps_obj: ID3D11PixelShader,
}

pub fn create_program(gpu: &Gpu, vs: Compiled, ps: Compiled) -> Result<Program, String> {
    let (mut v, mut p) = (None, None);
    // SAFETY: bytecode slices are valid for the calls; out-pointers are locals.
    unsafe {
        gpu.device
            .CreateVertexShader(&vs.bytecode, None, Some(&mut v))
            .map_err(|e| gpu.fail("CreateVertexShader", e))?;
        gpu.device
            .CreatePixelShader(&ps.bytecode, None, Some(&mut p))
            .map_err(|e| gpu.fail("CreatePixelShader", e))?;
    }
    Ok(Program {
        vs_obj: v.ok_or("no vertex shader")?,
        ps_obj: p.ok_or("no pixel shader")?,
        vs,
        ps,
    })
}

/// Which reflected SRV kind lives at `reg`, if the PS reads it.
pub fn srv_kind_at(r: &Reflection, reg: u32) -> Option<SrvKind> {
    r.srvs.iter().find(|s| s.1 == reg).map(|s| s.2)
}

impl ShapeDraw {
    /// Writes the scene into every cbuffer and uploads them.
    pub fn update(&mut self, gpu: &Gpu, scene: &Scene, view: &View) {
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            scene.write(&mut cb.writer, view, &self.model);
            // SAFETY: the buffer was created with exactly `bytes().len()` bytes (≥ 16).
            unsafe {
                gpu.ctx.UpdateSubresource(
                    &cb.buffer,
                    0,
                    None,
                    cb.writer.bytes().as_ptr() as _,
                    0,
                    0,
                )
            };
        }
    }

    /// A cbuffer edit (03 §1.3): every writer back to zero, then the material, so no field of the previous
    /// material survives. The scene fields are written again by the next [`ShapeDraw::update`].
    pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool) {
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            cb.writer.zero();
            write_material(&mut cb.writer, m, &self.bound, linear_lighting);
        }
        self.blend = m.blend();
    }
}
