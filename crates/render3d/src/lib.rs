pub mod cbuffer;
pub mod compile;
pub mod device;
pub mod draw;
pub mod includes;
pub mod locate;
pub mod mesh;
pub mod parse;
pub mod permutation;
pub mod scene;
pub mod textures;

pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};

use compile::{compile, SrvKind};
use device::{Gpu, Target};
use draw::{cbuffers, create_program, write_material, MaterialInputs, Program, ShapeDraw};
use locate::Fingerprint;
use permutation::{defines, Material, Stage};
use scene::{Scene, View};
use std::collections::HashMap;
use std::path::PathBuf;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Query, ID3D11RasterizerState, ID3D11SamplerState,
    ID3D11ShaderResourceView, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
    D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA, D3D11_COLOR_WRITE_ENABLE_ALL,
    D3D11_COMPARISON_NEVER, D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_FLOAT32_MAX, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_RASTERIZER_DESC,
    D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_WRAP, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16_UINT;

/// CS feature defines compiled by default. The owner's installed CS 1.9 has Linear Lighting on
/// (answers.md: `SettingsUser.json` `enableLinearLighting: 1`), and Linear Lighting is the compile-time
/// define `ENABLE_LL` (`Common/ColorManagement.hlsli:13,133`). P0 does no other installed-feature detection;
/// the GUI's "Community Shaders features" toggle clears this list (`Renderer::set_features`).
///
/// This is the owner's CS SettingsUser.json / frame-11178 value; P1 reads the enabled features per user.
pub const DEFAULT_FEATURES: &[&str] = &["ENABLE_LL"];

/// Result of [`Renderer::load`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadReport {
    /// BSVER 83: nothing is rendered (00 §6.5).
    pub le_mesh: bool,
    /// Shapes that will be drawn.
    pub shapes: usize,
    /// Texture paths (canonical `textures/…`) the resolver did not return; their registers get defaults.
    pub missing_textures: Vec<String>,
    /// Per-shape permutation compile errors (the shape is skipped, the others still draw).
    pub compile_errors: Vec<String>,
    /// Other per-shape problems: NIF scan/geometry errors, an invalid `Mesh`, unreadable DDS, GPU errors.
    pub errors: Vec<String>,
    /// Non-fatal notes: skipped non-Lighting shapes, cbuffer names this CS build lacks (logged once).
    pub warnings: Vec<String>,
}

/// Where the time of the last [`Renderer::render`] went, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameTiming {
    /// CPU time to record the draws (cbuffer updates, state, `DrawIndexed`).
    pub draw_ms: f64,
    /// Waiting for the GPU to finish the draws (an event query after a flush).
    pub gpu_ms: f64,
    /// Copy to a staging texture, `Map`, and the half → RGBA8 conversion; the GPU is idle by then, so this is
    /// the readback cost alone (the B9 zero-copy gate).
    pub readback_ms: f64,
}

/// The 3D preview renderer: message-shaped (load bytes → report, render scene → pixels) so it can later move
/// into a sidecar process. Single-threaded, like [`Gpu`].
pub struct Renderer {
    gpu: Gpu,
    shaders: Fingerprint,
    cache_dir: PathBuf,
    features: Vec<String>,
    programs: HashMap<String, Result<Program, String>>,
    defaults: HashMap<(SrvKind, [u8; 4]), ID3D11ShaderResourceView>,
    shapes: Vec<ShapeDraw>,
    bounds: Option<([f32; 3], f32)>,
    sampler: ID3D11SamplerState,
    raster: ID3D11RasterizerState,
    alpha_blend: ID3D11BlendState,
    target: Option<Target>,
    encode_srgb: bool,
    timing: FrameTiming,
}

fn permutation_key(d: &permutation::Defines, target: &str) -> String {
    let mut v: Vec<String> = d.0.iter().map(|(k, v)| format!("{k}={v}")).collect();
    v.sort();
    format!("{target}|{}", v.join(";"))
}

fn bit(v: u32, b: u32) -> bool {
    v & (1 << b) != 0
}

impl Renderer {
    /// Creates the device (hardware, else WARP; see [`Gpu::new`]) and the fixed pipeline states.
    pub fn new(shaders: Fingerprint, cache_dir: PathBuf) -> Result<Renderer, String> {
        Renderer::with_gpu(Gpu::new(None)?, shaders, cache_dir)
    }

    pub fn with_gpu(
        gpu: Gpu,
        shaders: Fingerprint,
        cache_dir: PathBuf,
    ) -> Result<Renderer, String> {
        let sd = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        // Cull none: the forward capture draw (state_6544 Arch_Hair) has CullMode.NoCull, and P0 does not
        // model the per-shape Double_Sided flag.
        // Front = counter-clockwise: the captured camera basis (FrameBuffer::CameraView rows right = f × up,
        // up, forward; reproduced by `View::new`) mirrors the Z-up world, and the captured opaque draws use
        // CullMode.Back (state_4217.json) yet show their outer faces, so the engine's front faces are CCW.
        // With D3D's default (CW) every visible face reports SV_IsFrontFace = false and Lighting.hlsl flips
        // its TBN ("Fix incorrect vertex normals on double-sided meshes", iLH ~954): lit sides go dark.
        let rd = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: true.into(),
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut bd = D3D11_BLEND_DESC::default();
        bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let (mut s, mut r, mut b) = (None, None, None);
        // SAFETY: descriptors and out-pointers are locals.
        unsafe {
            gpu.device
                .CreateSamplerState(&sd, Some(&mut s))
                .map_err(|e| format!("CreateSamplerState: {e}"))?;
            gpu.device
                .CreateRasterizerState(&rd, Some(&mut r))
                .map_err(|e| format!("CreateRasterizerState: {e}"))?;
            gpu.device
                .CreateBlendState(&bd, Some(&mut b))
                .map_err(|e| format!("CreateBlendState: {e}"))?;
        }
        Ok(Renderer {
            gpu,
            shaders,
            cache_dir,
            features: DEFAULT_FEATURES.iter().map(|s| s.to_string()).collect(),
            programs: HashMap::new(),
            defaults: HashMap::new(),
            shapes: vec![],
            bounds: None,
            sampler: s.ok_or("no sampler")?,
            raster: r.ok_or("no rasterizer state")?,
            alpha_blend: b.ok_or("no blend state")?,
            target: None,
            encode_srgb: false,
            timing: FrameTiming::default(),
        })
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// CS feature defines for the next [`Renderer::load`] (default [`DEFAULT_FEATURES`]).
    pub fn set_features(&mut self, features: Vec<String>) {
        self.features = features;
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    /// Replaces the shaders (a moved or updated CS) and drops every compiled program: the in-memory cache is keyed
    /// by defines only. The loaded model still uses the old programs until the next [`Renderer::load`].
    pub fn set_shaders(&mut self, shaders: Fingerprint) {
        self.shaders = shaders;
        self.programs.clear();
    }

    pub fn shaders(&self) -> &Fingerprint {
        &self.shaders
    }

    /// sRGB-encode the frame on readback (off by default). Turn it on for display when the features include
    /// `ENABLE_LL`: Linear Lighting draws in linear working space; without it the output is already gamma-space.
    pub fn set_encode_srgb(&mut self, on: bool) {
        self.encode_srgb = on;
    }

    /// Timing of the last successful [`Renderer::render`].
    pub fn last_timing(&self) -> FrameTiming {
        self.timing
    }

    /// Bounding sphere (centre, radius) of every drawn shape's `positions × transform` (skinned shapes:
    /// positions as-is, since the skinned VS ignores World and P0 bones are identity). Parent NiNode
    /// transforms are not applied in P0.
    pub fn bounds(&self) -> Option<([f32; 3], f32)> {
        self.bounds
    }

    fn program(&mut self, m: &Material) -> Result<Program, String> {
        let f: Vec<&str> = self.features.iter().map(String::as_str).collect();
        let vd = defines(m, Stage::Vertex, &f, false);
        let pd = defines(m, Stage::Pixel, &f, false);
        let key = format!(
            "{}#{}",
            permutation_key(&vd, "vs_5_0"),
            permutation_key(&pd, "ps_5_0")
        );
        if let Some(p) = self.programs.get(&key) {
            return p.clone();
        }
        let root = self.shaders.root.clone();
        let file = root.join("Lighting.hlsl");
        let (cache, hash) = (&self.cache_dir, &self.shaders.hash);
        let built = compile(&file, &root, &vd, "vs_5_0", cache, hash)
            .map_err(|e| format!("vs: {e}"))
            .and_then(|vs| {
                compile(&file, &root, &pd, "ps_5_0", cache, hash)
                    .map_err(|e| format!("ps: {e}"))
                    .map(|ps| (vs, ps))
            })
            .and_then(|(vs, ps)| create_program(&self.gpu, vs, ps));
        self.programs.insert(key, built.clone());
        built
    }

    fn default_srv(
        &mut self,
        kind: SrvKind,
        rgba: [u8; 4],
    ) -> Result<ID3D11ShaderResourceView, String> {
        if let Some(v) = self.defaults.get(&(kind, rgba)) {
            return Ok(v.clone());
        }
        let v = textures::default_srv(&self.gpu, kind, rgba)?;
        self.defaults.insert((kind, rgba), v.clone());
        Ok(v)
    }

    /// Parses the NIF, builds a draw per BSLightingShaderProperty shape and replaces the loaded model.
    /// `textures` resolves a canonical texture path (`textures/…`, lower case, `/`) to DDS bytes.
    pub fn load(
        &mut self,
        nif_bytes: &[u8],
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.shapes.clear();
        self.bounds = None;
        let mut rep = LoadReport::default();
        let scan = match nif::scan::scan(nif_bytes) {
            Ok(s) => s,
            Err(e) => {
                rep.errors.push(format!("NIF: {e}"));
                return rep;
            }
        };
        rep.warnings
            .extend(scan.problems.iter().map(|p| format!("NIF: {p}")));
        if scan.le_mesh {
            rep.le_mesh = true;
            return rep;
        }
        let header = match nif::header::Header::parse(nif_bytes) {
            Ok(h) => h,
            Err(e) => {
                rep.errors.push(format!("NIF header: {e}"));
                return rep;
            }
        };
        let linear_lighting = self.features.iter().any(|f| f == "ENABLE_LL");
        let mut tex_cache: HashMap<String, Option<textures::Texture>> = HashMap::new();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut points: Vec<[f32; 3]> = vec![];
        for row in &scan.shapes {
            let label = row
                .head
                .name
                .clone()
                .unwrap_or_else(|| format!("block {}", row.block));
            let nif::scan::Shader::Lighting(lighting, slots) = &row.shader else {
                rep.warnings.push(format!(
                    "{label}: not a BSLightingShaderProperty shape, not drawn in P0"
                ));
                continue;
            };
            let mesh = match nif::geom::read_shape(&header, nif_bytes, row.block as usize) {
                Ok(m) => m,
                Err(e) => {
                    rep.errors.push(format!("{label}: {e}"));
                    continue;
                }
            };
            if let Err(e) = mesh::validate(&mesh) {
                rep.errors.push(format!("{label}: {e}"));
                continue;
            }
            let material = Material::from_shape(&row.head, lighting, row.alpha);
            let program = match self.program(&material) {
                Ok(p) => p,
                Err(e) => {
                    rep.compile_errors.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let pbr = bit(lighting.slsf2, 23);
            let msn = bit(lighting.slsf1, 12);
            let blend = row.alpha.is_some_and(|a| a.blend());
            let ps_defines = &program.ps.reflection;
            let specular =
                !pbr && bit(lighting.slsf1, 0) && lighting.params.specular_strength > 0.0;
            // NIF slot → register; a texture is loaded only if the PS reads its register.
            let mut bound: HashMap<u32, ID3D11ShaderResourceView> = HashMap::new();
            for (slot, path) in slots.iter().enumerate() {
                if path.norm.is_empty() {
                    continue;
                }
                let Some(reg) = textures::register_for_slot(lighting.shader_type, pbr, msn, slot)
                else {
                    continue;
                };
                let Some(kind) = draw::srv_kind_at(ps_defines, reg) else {
                    continue;
                };
                if !tex_cache.contains_key(&path.norm) {
                    let loaded = match textures(&path.norm) {
                        None => {
                            if !rep.missing_textures.contains(&path.norm) {
                                rep.missing_textures.push(path.norm.clone());
                            }
                            None
                        }
                        Some(bytes) => match textures::load_dds(&self.gpu, &bytes) {
                            Ok(t) => Some(t),
                            Err(e) => {
                                rep.errors.push(format!("{}: {e}", path.norm));
                                None
                            }
                        },
                    };
                    tex_cache.insert(path.norm.clone(), loaded);
                }
                match tex_cache.get(&path.norm) {
                    Some(Some(t)) if t.kind == kind => {
                        bound.insert(reg, t.srv.clone());
                    }
                    Some(Some(_)) => rep.warnings.push(format!(
                        "{label}: {} is not a {kind:?} texture (t{reg}); default bound",
                        path.norm
                    )),
                    _ => {}
                }
            }
            let mut srvs = vec![];
            let mut srv_err = None;
            for (_, reg, kind) in &program.ps.reflection.srvs {
                if let Some(v) = bound.get(reg) {
                    srvs.push((*reg, v.clone()));
                    continue;
                }
                let rgba = textures::default_for(*reg, lighting.shader_type, pbr);
                match self.default_srv(*kind, rgba) {
                    Ok(v) => srvs.push((*reg, v)),
                    Err(e) => srv_err = Some(e),
                }
            }
            if let Some(e) = srv_err {
                rep.errors.push(format!("{label}: default texture: {e}"));
                continue;
            }
            let gpu_mesh = match mesh::build(
                &self.gpu,
                &mesh,
                &program.vs.reflection.inputs,
                &program.vs.bytecode,
            ) {
                Ok(m) => m,
                Err(e) => {
                    rep.errors.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let (mut vs_cbs, mut ps_cbs) = match (
                cbuffers(&self.gpu, &program.vs.reflection),
                cbuffers(&self.gpu, &program.ps.reflection),
            ) {
                (Ok(v), Ok(p)) => (v, p),
                (Err(e), _) | (_, Err(e)) => {
                    rep.errors.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let inputs = MaterialInputs {
                lighting,
                alpha: row.alpha,
                material,
                pbr,
                specular,
                linear_lighting,
                blend,
                has_env_mask: bound.contains_key(&5),
            };
            for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
                write_material(&mut cb.writer, &inputs);
            }
            let model = mesh.transform;
            for p in &mesh.positions {
                let q = if material.skinned {
                    *p
                } else {
                    let t = &model;
                    [0, 1, 2].map(|r| t[r][0] * p[0] + t[r][1] * p[1] + t[r][2] * p[2] + t[r][3])
                };
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
                points.push(q);
            }
            self.shapes.push(ShapeDraw {
                name: label,
                vs: program.vs_obj.clone(),
                ps: program.ps_obj.clone(),
                mesh: gpu_mesh,
                vs_cbs,
                ps_cbs,
                srvs,
                samplers: program.ps.reflection.samplers.iter().map(|s| s.1).collect(),
                model,
                blend,
                skinned: material.skinned,
            });
        }
        if !points.is_empty() {
            let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
            let r = points
                .iter()
                .map(|p| {
                    ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt()
                })
                .fold(0.0f32, f32::max);
            self.bounds = Some((c, r));
        }
        // A required name no drawn shape's cbuffers have: logged once per build (CS version drift, 03 §6).
        // The skinned VS reads neither World nor EyePosition (bones + BonesPivot instead), so those two are
        // only expected from non-skinned shapes.
        if !self.shapes.is_empty() {
            for n in scene::REQUIRED {
                let world_only = matches!(*n, "World" | "EyePosition");
                let candidates: Vec<&ShapeDraw> = self
                    .shapes
                    .iter()
                    .filter(|s| !(world_only && s.skinned))
                    .collect();
                if candidates.is_empty() {
                    continue;
                }
                let somewhere = candidates
                    .iter()
                    .any(|s| s.vs_cbs.iter().chain(&s.ps_cbs).any(|cb| cb.writer.has(n)));
                if !somewhere {
                    rep.warnings
                        .push(format!("this CS build has no cbuffer field {n:?}"));
                }
            }
        }
        // Opaque first, then alpha-blended (P0 does not sort blended shapes by depth).
        self.shapes.sort_by_key(|s| s.blend);
        rep.shapes = self.shapes.len();
        rep
    }

    /// Renders the loaded model into a `w × h` RGBA8 frame (tightly packed, top row first, alpha 255: see
    /// [`Target::readback_display`]). Errors (device
    /// removed, allocation failure) are text; see [`device`] for the device-removed contract.
    pub fn render(&mut self, scene: &Scene, w: u32, h: u32) -> Result<Vec<u8>, String> {
        let t0 = std::time::Instant::now();
        let (w, h) = (w.max(1), h.max(1));
        if self
            .target
            .as_ref()
            .is_none_or(|t| t.width != w || t.height != h)
        {
            self.target = Some(Target::new(&self.gpu, w, h)?);
        }
        let view = View::new(&scene.camera, self.bounds, w, h);
        let gpu = &self.gpu;
        let target = self.target.as_ref().ok_or("no target")?;
        target.clear(gpu, [0.0, 0.0, 0.0, 1.0]);
        // SAFETY (whole block): every bound object belongs to `gpu.device`; slices outlive the calls.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(Some(&[Some(target.rtv.clone())]), &target.dsv);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            ctx.RSSetState(&self.raster);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            for s in self.shapes.iter_mut() {
                s.update(gpu, scene, &view);
                ctx.IASetInputLayout(&s.mesh.layout);
                for (slot, (b, stride)) in s.mesh.streams.iter().enumerate() {
                    ctx.IASetVertexBuffers(
                        slot as u32,
                        1,
                        Some(&Some(b.clone())),
                        Some(stride),
                        Some(&0),
                    );
                }
                ctx.IASetIndexBuffer(&s.mesh.index, DXGI_FORMAT_R16_UINT, 0);
                ctx.VSSetShader(&s.vs, None);
                ctx.PSSetShader(&s.ps, None);
                for cb in &s.vs_cbs {
                    ctx.VSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
                }
                for cb in &s.ps_cbs {
                    ctx.PSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
                }
                for (reg, v) in &s.srvs {
                    ctx.PSSetShaderResources(*reg, Some(&[Some(v.clone())]));
                }
                for slot in &s.samplers {
                    ctx.PSSetSamplers(*slot, Some(&[Some(self.sampler.clone())]));
                }
                if s.blend {
                    ctx.OMSetBlendState(&self.alpha_blend, None, 0xffff_ffff);
                } else {
                    ctx.OMSetBlendState(None, None, 0xffff_ffff);
                }
                ctx.DrawIndexed(s.mesh.index_count, 0, 0);
            }
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
        }
        let draw_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        wait_idle(gpu)?;
        let gpu_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let t2 = std::time::Instant::now();
        let px = target.readback_display(gpu, self.encode_srgb)?;
        self.timing = FrameTiming {
            draw_ms,
            gpu_ms,
            readback_ms: t2.elapsed().as_secs_f64() * 1000.0,
        };
        Ok(px)
    }
}

/// Flushes and spins on an event query until the GPU has finished every submitted command, so the readback
/// that follows measures the copy alone. A failing query (removed device) is an error with the device prefix.
fn wait_idle(gpu: &Gpu) -> Result<(), String> {
    let desc = D3D11_QUERY_DESC {
        Query: D3D11_QUERY_EVENT,
        MiscFlags: 0,
    };
    let mut q: Option<ID3D11Query> = None;
    // SAFETY: `desc` and the out-pointer are locals; the query belongs to `gpu.device`.
    unsafe {
        gpu.device
            .CreateQuery(&desc, Some(&mut q))
            .map_err(|e| gpu.fail("CreateQuery", e))?;
    }
    let q = q.ok_or_else(|| gpu.fail("CreateQuery", "no query returned"))?;
    // SAFETY: `done` is a live BOOL-sized local, the size passed matches it.
    unsafe {
        gpu.ctx.End(&q);
        gpu.ctx.Flush();
        let start = std::time::Instant::now();
        loop {
            let mut done = windows::core::BOOL(0);
            gpu.ctx
                .GetData(
                    &q,
                    Some(&mut done as *mut _ as *mut core::ffi::c_void),
                    std::mem::size_of::<windows::core::BOOL>() as u32,
                    0,
                )
                .map_err(|e| gpu.fail("GetData", e))?;
            if done.as_bool() {
                return Ok(());
            }
            if start.elapsed().as_secs() > 10 {
                // The readback's Map waits anyway; the timing is then approximate.
                return Ok(());
            }
            std::thread::yield_now();
        }
    }
}
