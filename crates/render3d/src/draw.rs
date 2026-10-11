//! One NIF shape → one forward CS Lighting draw: permutation, VS/PS, zero-initialised cbuffers filled from
//! the NIF material and the stub scene, SRVs for every reflected register, one sampler per reflected slot.
//! P0 is forward only (no `DEFERRED`, no 8 MRTs); in-game opaque objects take the deferred path
//! (answers.md, hint 15), but the forward permutation of the same Lighting.hlsl is what P0 renders.
use crate::cbuffer::CbWriter;
use crate::compile::{Compiled, Reflection, SrvKind};
use crate::device::Gpu;
use crate::mesh::GpuMesh;
use crate::permutation::{runtime_bits, Material};
use crate::scene::{srgb_to_linear, Mat3x4, Scene, View};
use nif::props::{Alpha, Lighting, LightingParams, LightingTrailer};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_USAGE_DEFAULT,
};

/// `PerMaterial.SpecularColor` as the engine uploads it (answers.md Q5, frame11178 ev4217/5488/5536/6544):
/// `xyz = sRGB→linear(NIF specular colour) × NIF specular strength`, `w = NIF glossiness` (shininess,
/// iLH:973). The linearisation is the Linear Lighting effect (owner's CS has LL on); with LL off it is
/// `colour × strength` [Inference, Q5 open item]. Written only when SPECULAR is compiled; the PBR path
/// uses [`pbr_params`] instead.
pub fn pack_specular(p: &LightingParams, linear_lighting: bool) -> [f32; 4] {
    let c = p.specular_color.map(|c| {
        if linear_lighting {
            srgb_to_linear(c)
        } else {
            c
        }
    });
    let s = p.specular_strength;
    [c[0] * s, c[1] * s, c[2] * s, p.glossiness]
}

/// TruePBR `(PBRParams1, PBRParams2)` (answers.md Q5, Arch_Eye ev3891 = NIF exactly):
/// `PBRParams1 = (specStrength → roughness scale, lightingEffect2 → displacement scale, glossiness →
/// specular level)`, `PBRParams2 = (specular colour raw — not linearised —, lightingEffect1)`.
pub fn pbr_params(p: &LightingParams) -> ([f32; 3], [f32; 4]) {
    (
        [p.specular_strength, p.lighting_effect_2, p.glossiness],
        [
            p.specular_color[0],
            p.specular_color[1],
            p.specular_color[2],
            p.lighting_effect_1,
        ],
    )
}

/// The material inputs of one shape.
pub struct MaterialInputs<'a> {
    pub lighting: &'a Lighting,
    pub alpha: Option<Alpha>,
    pub material: Material,
    pub pbr: bool,
    pub specular: bool,
    pub linear_lighting: bool,
    pub blend: bool,
    pub has_env_mask: bool,
}

/// Writes the NIF-derived fields into one (zero-initialised) cbuffer of any stage.
pub fn write_material(w: &mut CbWriter, m: &MaterialInputs) {
    let p = &m.lighting.params;
    w.set(
        "TexcoordOffset",
        &[p.uv_offset[0], p.uv_offset[1], p.uv_scale[0], p.uv_scale[1]],
    );
    if m.specular {
        w.set("SpecularColor", &pack_specular(p, m.linear_lighting));
    }
    if m.pbr {
        let (p1, p2) = pbr_params(p);
        w.set("PBRParams1", &p1);
        w.set("PBRParams2", &p2);
    }
    w.set(
        "LightingEffectParams",
        &[p.lighting_effect_1, p.lighting_effect_2, 0.0, 0.0],
    );
    w.set("MaterialObjectRGBScale", &[1.0, 1.0, 1.0]);
    // TintColor is not linearised (answers.md Q5: hair 6544 = NIF hairTint).
    match &p.trailer {
        LightingTrailer::SkinTint(c) | LightingTrailer::HairTint(c) => {
            w.set("TintColor", &[c[0], c[1], c[2], 1.0]);
        }
        LightingTrailer::EnvMap { scale } => {
            w.set(
                "EnvmapData",
                &[*scale, if m.has_env_mask { 1.0 } else { 0.0 }, 0.0, 0.0],
            );
        }
        LightingTrailer::ParallaxOcc { max_passes, scale } => {
            w.set("ParallaxOccData", &[*max_passes, *scale, 0.0, 0.0]);
        }
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            envmap_strength,
        } => {
            w.set(
                "MultiLayerParallaxData",
                &[
                    *inner_thickness,
                    *refraction_scale,
                    inner_texture_scale[0],
                    inner_texture_scale[1],
                ],
            );
            w.set(
                "EnvmapData",
                &[
                    *envmap_strength,
                    if m.has_env_mask { 1.0 } else { 0.0 },
                    0.0,
                    0.0,
                ],
            );
        }
        LightingTrailer::Eye {
            left_center,
            right_center,
            cubemap_scale,
        } => {
            w.set(
                "LeftEyeCenter",
                &[left_center[0], left_center[1], left_center[2], 0.0],
            );
            w.set(
                "RightEyeCenter",
                &[right_center[0], right_center[1], right_center[2], 0.0],
            );
            w.set(
                "EnvmapData",
                &[
                    *cubemap_scale,
                    if m.has_env_mask { 1.0 } else { 0.0 },
                    0.0,
                    0.0,
                ],
            );
        }
        LightingTrailer::SparkleSnow(s) => {
            w.set("SparkleParams", s);
        }
        LightingTrailer::None => {}
    }
    // EmitColor: "sRGB by default, linear if LL on" (iLH ~2595); added to diffuse for every vanilla draw, so
    // only shapes with Own_Emit (SLSF1.22) get the NIF colour × multiple [Inference: every captured
    // EmitColor was 0].
    let emit = if m.material.slsf1 & (1 << 22) != 0 {
        p.emissive_color.map(|c| {
            (if m.linear_lighting {
                srgb_to_linear(c)
            } else {
                c
            }) * p.emissive_multiple
        })
    } else {
        [0.0; 3]
    };
    w.set("EmitColor", &emit);
    w.set("emissiveMult", &[p.emissive_multiple]);
    // envmap LOD fade, specular LOD fade, alpha.
    w.set("MaterialData", &[1.0, 1.0, p.alpha, 0.0]);
    // b11 AlphaTestRefRS = threshold/255 only when the NiAlphaProperty test flag is set; b2 AlphaTestRef stays
    // 0 (frame11178: Hair ev6544 test 40 → 0.1569, Lashes ev5536 test 5 → 0.0196, blend-only Scalp ev5488
    // threshold 50 → 0; b2 AlphaTestRef = 0 on all of them). Blend-only shapes still compile DO_ALPHA_TEST.
    let tref = m
        .alpha
        .filter(|a| a.test())
        .map_or(0.0, |a| a.threshold as f32 / 255.0);
    w.set("AlphaTestRefRS", &[tref]);
    // cb4: engine PS desc & ~compile key = runtime bits only (answers.md Q4.1, B5 `runtime_bits`).
    w.set_u32("PixelShaderDescriptor", &[runtime_bits(&m.material)]);
    // InWorld (bit 0); SourceAlphaBlend (bit 8) only for blended passes (answers.md Q4.2).
    w.set_u32(
        "ExtraShaderDescriptor",
        &[1 | if m.blend { 1 << 8 } else { 0 }],
    );
    // Identity bones for all 240 rows (P0 bind pose); the camera-relative shift is BonesPivot (Scene).
    let bones: Vec<f32> = (0..80)
        .flat_map(|_| [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .collect();
    w.set("Bones", &bones);
    w.set("PreviousBones", &bones);
}

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
                .map_err(|e| format!("CreateBuffer({}): {e}", l.name))?;
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
            .map_err(|e| format!("CreateVertexShader: {e}"))?;
        gpu.device
            .CreatePixelShader(&ps.bytecode, None, Some(&mut p))
            .map_err(|e| format!("CreatePixelShader: {e}"))?;
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
}
