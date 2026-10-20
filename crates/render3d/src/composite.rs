//! CS's DeferredCompositeCS (v2 Phase D, D4): compiled cs_5_0 from the user's install, run after the opaque DEFERRED
//! draws with the MRTs as SRVs (bound by reflected name) and RT0 as MainRW. Every A / A_RT golden capture binds only
//! t0/t2 (t4 null) with b5 + b12, so under Linear Lighting the composite is `MainRW.rgb += Specular; MainRW.a = 1`.
//! Reflected SRVs/UAVs outside [`INPUTS`] and MainRW stay unbound (reads 0, writes dropped), as the capture had
//! DepthTexture; their names are in [`Composite::unbound`].
use crate::compile::{compile, Reflection, SrvKind};
use crate::cs_settings;
use crate::device::{Gpu, Target};
use crate::draw::{cbuffers, upload, Cb};
use crate::scene::{Mat3x4, Scene, View};
use crate::source::SourceSet;
use std::collections::BTreeSet;
use std::path::Path;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ComputeShader, D3D11_FEATURE_DATA_FORMAT_SUPPORT2, D3D11_FEATURE_FORMAT_SUPPORT2,
    D3D11_FORMAT_SUPPORT2_UAV_TYPED_LOAD,
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
const IDENT: Mat3x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

pub struct Composite {
    cs: ID3D11ComputeShader,
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
    /// MainRW, and every [`INPUTS`] name it reflects a 2D texture.
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
            match input(name) {
                Some(_) if *kind != SrvKind::Tex2D => {
                    return Err(format!("{SOURCE}: {name} is {kind:?}, not a 2D texture"))
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
        Ok(Composite {
            cs: cs.ok_or("no compute shader")?,
            cbs: cbuffers(gpu, &r)?,
            reflection: r,
            defines: defines.to_vec(),
            main_slot,
            unbound,
        })
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

    /// Runs the composite on `t` (deferred): unbinds the OM (RT0 becomes the UAV, RT2/RT4 SRVs), binds, dispatches
    /// and unbinds again. The caller rebinds its render targets.
    pub fn dispatch(&self, gpu: &Gpu, t: &Target) -> Result<(), String> {
        let uav = t
            .main_uav
            .as_ref()
            .filter(|_| t.deferred())
            .ok_or("the composite needs a deferred target")?;
        let srvs: Vec<(u32, usize)> = self
            .reflection
            .srvs
            .iter()
            .filter_map(|(n, reg, _)| Some((*reg, input(n)?)))
            .collect();
        let [x, y, z] = self.groups(t.width, t.height);
        // SAFETY (whole block): every bound object belongs to `gpu.device`; slices outlive the calls.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(None, None);
            ctx.CSSetShader(&self.cs, None);
            for cb in &self.cbs {
                ctx.CSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
            }
            for (reg, rt) in &srvs {
                ctx.CSSetShaderResources(*reg, Some(&[Some(t.mrt[*rt].2.clone())]));
            }
            ctx.CSSetUnorderedAccessViews(self.main_slot, 1, Some(&Some(uav.clone())), None);
            ctx.Dispatch(x, y, z);
            for (reg, _) in &srvs {
                ctx.CSSetShaderResources(*reg, Some(&[None]));
            }
            ctx.CSSetUnorderedAccessViews(self.main_slot, 1, Some(&None), None);
            ctx.CSSetShader(None, None);
        }
        Ok(())
    }
}
