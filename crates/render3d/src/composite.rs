//! CS's DeferredCompositeCS (v2 Phase D, D4): compiled cs_5_0 from the user's install, run after the opaque DEFERRED
//! draws with the MRTs as SRVs (bound by reflected name) and RT0 as MainRW. Every A / A_RT golden capture binds only
//! t0/t2 (t4 null) with b5 + b12, so under Linear Lighting the composite is `MainRW.rgb += Specular; MainRW.a = 1`.
//! Reflected SRVs/UAVs outside [`INPUTS`], [`Inputs`] and MainRW stay unbound (reads 0, writes dropped); their names
//! are in [`Composite::unbound`].
//!
//! D5: DepthTexture, the Dynamic Cubemaps cubes (EnvTexture, EnvReflectionsTexture) and the sampler come from
//! [`Inputs`]: the GUI passes the deferred depth and the HDRI cube ([`EnvMaps`]), the replay the captured ones. A
//! reflected input that is not given is an error (fail closed).
use crate::compile::{compile, Reflection, SrvKind};
use crate::cs_settings;
use crate::device::{Gpu, Target};
use crate::draw::{cbuffers, upload, Cb};
use crate::hdri::{self, Hdr};
use crate::scene::{Mat3x4, Scene, View};
use crate::source::SourceSet;
use std::collections::BTreeSet;
use std::path::Path;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ComputeShader, ID3D11SamplerState, ID3D11ShaderResourceView, D3D11_COMPARISON_NEVER,
    D3D11_FEATURE_DATA_FORMAT_SUPPORT2, D3D11_FEATURE_FORMAT_SUPPORT2,
    D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_FLOAT32_MAX, D3D11_FORMAT_SUPPORT2_UAV_TYPED_LOAD,
    D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;

pub const SOURCE: &str = "DeferredCompositeCS.hlsl";
/// Reflected composite SRV name → MRT index (DeferredCompositeCS.hlsl:11-15,30; Lighting.hlsl:290-301).
pub const INPUTS: [(&str, usize); 6] = [
    ("SpecularTexture", 4),
    ("AlbedoTexture", 3),
    ("NormalRoughnessTexture", 2),
    ("MasksTexture", 6),
    ("ReflectanceTexture", 5),
    ("Masks2Texture", 7),
];
/// Reflected composite SRVs that come from [`Inputs`], with the kind each must have (DeferredCompositeCS.hlsl:23-35).
const NAMED: [(&str, SrvKind); 3] = [
    ("DepthTexture", SrvKind::Tex2D),
    ("EnvTexture", SrvKind::Cube),
    ("EnvReflectionsTexture", SrvKind::Cube),
];
const IDENT: Mat3x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// The Dynamic Cubemaps cubes: `env` = EnvTexture, `reflection` = EnvReflectionsTexture.
pub struct EnvMaps {
    pub env: ID3D11ShaderResourceView,
    pub reflection: ID3D11ShaderResourceView,
}

impl EnvMaps {
    /// From the HDRI turned by `yaw`: [`hdri::cube_faces`] + [`hdri::mip_chain`] down to 1×1 (the composite divides by
    /// the level-15 luminance), RGBA16F. `env` and `reflection` are the same view (an HDRI has no sky/no-sky split).
    pub fn new(gpu: &Gpu, img: &Hdr, size: u32, yaw: f32) -> Result<EnvMaps, String> {
        let mut data = Vec::new();
        let mut mips = 0;
        for face in hdri::cube_faces(img, size, yaw) {
            let chain = hdri::mip_chain(&face, size);
            mips = chain.len() as u32;
            for px in chain.iter().flatten() {
                // As the backdrop: clamped below f16's maximum, so a hot pixel never becomes infinity.
                data.extend(
                    px.iter()
                        .flat_map(|v| half::f16::from_f32(v.min(65000.0)).to_le_bytes()),
                );
            }
        }
        let v =
            crate::textures::cube_srv(gpu, size, mips, DXGI_FORMAT_R16G16B16A16_FLOAT, 8, &data)?;
        Ok(EnvMaps {
            env: v.clone(),
            reflection: v,
        })
    }
}

/// What the composite reads besides the target: the depth SRV, the env cubes, and its sampler (`None` = the
/// composite's own LinearSampler, CS's captured state).
pub struct Inputs<'a> {
    pub depth: Option<&'a ID3D11ShaderResourceView>,
    pub env: Option<&'a EnvMaps>,
    pub sampler: Option<&'a ID3D11SamplerState>,
}

pub struct Composite {
    cs: ID3D11ComputeShader,
    /// MIN_MAG_MIP_LINEAR, clamp, LOD 0..max: the captured LinearSampler (composite/extras.json samplers.cs).
    linear: ID3D11SamplerState,
    pub reflection: Reflection,
    /// What it was compiled with (render rebuilds when the wanted list differs).
    pub defines: Vec<String>,
    /// One per reflected cbuffer ([`cbuffers`]); replay swaps `buffer` for the captured bytes.
    pub cbs: Vec<Cb>,
    /// Reflected UAV "MainRW".
    main_slot: u32,
    /// Reflected SRVs/UAVs left unbound ("t4 DepthTexture", "u1 NormalTAAMaskSpecularMaskRW").
    pub unbound: Vec<String>,
}

fn input(name: &str) -> Option<usize> {
    INPUTS.iter().find(|i| i.0 == name).map(|i| i.1)
}

impl Composite {
    /// The composite reads MainRW (RGBA16F) through a UAV: optional at FL11_0.
    pub fn typed_uav_load(gpu: &Gpu) -> bool {
        let mut d = D3D11_FEATURE_DATA_FORMAT_SUPPORT2 {
            InFormat: DXGI_FORMAT_R16G16B16A16_FLOAT,
            OutFormatSupport2: 0,
        };
        // SAFETY: `d` is a live local of the size passed.
        let ok = unsafe {
            gpu.device.CheckFeatureSupport(
                D3D11_FEATURE_FORMAT_SUPPORT2,
                &mut d as *mut _ as _,
                std::mem::size_of_val(&d) as u32,
            )
        };
        ok.is_ok() && d.OutFormatSupport2 & D3D11_FORMAT_SUPPORT2_UAV_TYPED_LOAD.0 as u32 != 0
    }

    /// Compiles `SOURCE` (cs_5_0, `defines` as "K" or "K=V") and checks what render binds: thread-group size,
    /// MainRW, every [`INPUTS`] name it reflects a 2D texture, DepthTexture 2D and the env cubes cubes.
    pub fn new(
        gpu: &Gpu,
        src: &SourceSet,
        defines: &[String],
        cache_dir: &Path,
    ) -> Result<Composite, String> {
        if !Composite::typed_uav_load(gpu) {
            return Err(
                "this GPU cannot read R16G16B16A16_FLOAT through a UAV (typed UAV load)".into(),
            );
        }
        let s = src.get(Path::new(SOURCE))?;
        let c = compile(&s, &crate::replay::to_defines(defines), "cs_5_0", cache_dir)?;
        let r = c.reflection;
        if r.threads == [0; 3] {
            return Err(format!("{SOURCE}: no thread-group size"));
        }
        let main_slot = r
            .uavs
            .iter()
            .find(|u| u.0 == "MainRW")
            .ok_or_else(|| format!("{SOURCE}: no MainRW UAV"))?
            .1;
        let mut unbound = vec![];
        for (name, reg, kind) in &r.srvs {
            let want = match (input(name), NAMED.iter().find(|n| n.0 == name)) {
                (Some(_), _) => Some(SrvKind::Tex2D),
                (None, Some(n)) => Some(n.1),
                (None, None) => None,
            };
            match want {
                Some(k) if *kind != k => {
                    return Err(format!("{SOURCE}: {name} is {kind:?}, not {k:?}"))
                }
                Some(_) => {}
                None => unbound.push(format!("t{reg} {name}")),
            }
        }
        unbound.extend(
            r.uavs
                .iter()
                .filter(|u| u.0 != "MainRW")
                .map(|(n, reg)| format!("u{reg} {n}")),
        );
        let mut cs = None;
        // SAFETY: the bytecode slice is valid for the call; the out-pointer is a local.
        unsafe {
            gpu.device
                .CreateComputeShader(&c.bytecode, None, Some(&mut cs))
        }
        .map_err(|e| gpu.fail("CreateComputeShader", e))?;
        let sd = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        let mut linear = None;
        // SAFETY: descriptor and out-pointer are locals.
        unsafe { gpu.device.CreateSamplerState(&sd, Some(&mut linear)) }
            .map_err(|e| gpu.fail("CreateSamplerState", e))?;
        Ok(Composite {
            cs: cs.ok_or("no compute shader")?,
            linear: linear.ok_or("no sampler")?,
            cbs: cbuffers(gpu, &r)?,
            reflection: r,
            defines: defines.to_vec(),
            main_slot,
            unbound,
        })
    }

    /// The composite reads an env cube (DYNAMIC_CUBEMAPS): render builds [`EnvMaps`] for it.
    pub fn needs_env(&self) -> bool {
        self.reflection
            .srvs
            .iter()
            .any(|s| s.0 == "EnvTexture" || s.0 == "EnvReflectionsTexture")
    }

    /// The reflected cbuffer field names, namespace-stripped (what `cs_settings::feature_data` keys on).
    pub fn fields(&self) -> BTreeSet<String> {
        cs_settings::reflected_fields(&self.reflection.cbuffers)
    }

    /// The scene by name into every cbuffer (BufferDim, the camera inverses, DynamicResolutionParams2), uploaded.
    pub fn write_scene(
        &mut self,
        gpu: &Gpu,
        scene: &Scene,
        view: &View,
        feature_values: &[(String, Vec<f64>)],
    ) {
        for cb in &mut self.cbs {
            scene.write(&mut cb.writer, view, &IDENT, feature_values);
            upload(gpu, cb);
        }
    }

    /// Thread groups covering `w × h`.
    pub fn groups(&self, w: u32, h: u32) -> [u32; 3] {
        let [tx, ty, _] = self.reflection.threads;
        [w.div_ceil(tx.max(1)), h.div_ceil(ty.max(1)), 1]
    }

    /// Runs the composite on `t` (deferred): unbinds the OM (RT0 becomes the UAV, the MRTs and depth SRVs), binds by
    /// reflected name (MRTs, `inputs`, every reflected sampler), dispatches and unbinds again. A reflected input that
    /// `inputs` lacks is an Err before anything is bound. The caller rebinds its render targets.
    pub fn dispatch(&self, gpu: &Gpu, t: &Target, inputs: &Inputs) -> Result<(), String> {
        let uav = t
            .main_uav
            .as_ref()
            .filter(|_| t.deferred())
            .ok_or("the composite needs a deferred target")?;
        let missing =
            |name: &str, what: &str| format!("composite reads {name} but no {what} was given");
        let mut srvs: Vec<(u32, ID3D11ShaderResourceView)> = vec![];
        for (name, reg, _) in &self.reflection.srvs {
            let v = match (input(name), name.as_str()) {
                (Some(rt), _) => t.mrt[rt].2.clone(),
                (_, "DepthTexture") => inputs.depth.ok_or_else(|| missing(name, "depth"))?.clone(),
                (_, "EnvTexture") => inputs
                    .env
                    .ok_or_else(|| missing(name, "env cube"))?
                    .env
                    .clone(),
                (_, "EnvReflectionsTexture") => inputs
                    .env
                    .ok_or_else(|| missing(name, "env cube"))?
                    .reflection
                    .clone(),
                _ => continue,
            };
            srvs.push((*reg, v));
        }
        let sampler = inputs.sampler.unwrap_or(&self.linear);
        let [x, y, z] = self.groups(t.width, t.height);
        // SAFETY (whole block): every bound object belongs to `gpu.device`; slices outlive the calls.
        unsafe {
            let ctx = &gpu.ctx;
            // First: the depth SRV and RT0's UAV must not stay bound as DSV / RTV.
            ctx.OMSetRenderTargets(None, None);
            ctx.CSSetShader(&self.cs, None);
            for cb in &self.cbs {
                ctx.CSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
            }
            for (reg, v) in &srvs {
                ctx.CSSetShaderResources(*reg, Some(&[Some(v.clone())]));
            }
            for (_, slot) in &self.reflection.samplers {
                ctx.CSSetSamplers(*slot, Some(&[Some(sampler.clone())]));
            }
            ctx.CSSetUnorderedAccessViews(self.main_slot, 1, Some(&Some(uav.clone())), None);
            ctx.Dispatch(x, y, z);
            for (reg, _) in &srvs {
                ctx.CSSetShaderResources(*reg, Some(&[None]));
            }
            for (_, slot) in &self.reflection.samplers {
                ctx.CSSetSamplers(*slot, Some(&[None]));
            }
            ctx.CSSetUnorderedAccessViews(self.main_slot, 1, Some(&None), None);
            ctx.CSSetShader(None, None);
        }
        Ok(())
    }
}
