pub mod builtin;
pub mod cbuffer;
pub mod compile;
pub mod device;
pub mod draw;
pub mod includes;
pub mod locate;
pub mod material;
pub mod mesh;
pub mod parse;
pub mod permutation;
pub mod scene;
pub mod textures;

pub use builtin::{builtin_mesh, Builtin};
pub use material::{classify, pbr_flags, Bound, EditKind};
pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};

use compile::{compile, SrvKind};
use device::{Gpu, Target};
use draw::{cbuffers, create_program, Program, ShapeDraw};
use locate::Fingerprint;
use material::{perm_material, write_material};
use nif::material::MaterialState;
use permutation::{defines, Material, Stage};
use scene::{Scene, View};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
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

/// What a loaded shape is doing on screen. P1-B adds `Pending`.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeStatus {
    /// The current material is on screen.
    Drawing,
    /// `MaterialState::hidden` (PGPatcher `delete`): not drawn.
    Hidden,
    /// The current material could not be built (compile, mesh or GPU error). The previous draw, if there is
    /// one, stays on screen.
    Failed(String),
}

/// One loaded shape as the GUI sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeInfo {
    pub id: ShapeId,
    pub name: String,
    /// The material last loaded or set (even when it failed to build).
    pub material: MaterialState,
    pub status: ShapeStatus,
}

/// One loaded shape: the CPU mesh (kept so an edit rebuilds the draw without re-parsing), the material last
/// loaded or set, and what is on screen (possibly built from an earlier material, see [`ShapeStatus::Failed`]).
struct ShapeState {
    name: String,
    mesh: Arc<nif::geom::Mesh>,
    material: MaterialState,
    draw: Option<ShapeDraw>,
    status: ShapeStatus,
}

/// Why a draw could not be built. A permutation compile error is reported in `LoadReport::compile_errors`,
/// everything else in `errors`.
enum BuildError {
    Compile(String),
    Other(String),
}

impl BuildError {
    fn into_text(self) -> String {
        match self {
            BuildError::Compile(e) | BuildError::Other(e) => e,
        }
    }
}

/// A texture resolved once per load: its view and kind, or `None` (the resolver had no bytes, or they did
/// not decode).
type TexSlot = Option<(ID3D11ShaderResourceView, SrvKind)>;

/// The 3D preview renderer: message-shaped (load bytes → report, render scene → pixels) so it can later move
/// into a sidecar process. Single-threaded, like [`Gpu`].
pub struct Renderer {
    gpu: Gpu,
    shaders: Fingerprint,
    cache_dir: PathBuf,
    features: Vec<String>,
    programs: HashMap<String, Result<Program, String>>,
    defaults: HashMap<(SrvKind, [u8; 4]), ID3D11ShaderResourceView>,
    /// Every Lighting shape of the loaded model, keyed by NIF block index (built-ins: `ShapeId(0)`).
    shapes: BTreeMap<ShapeId, ShapeState>,
    /// Shapes with a draw that are not hidden: opaque first, then alpha-blended, block order within each.
    draw_order: Vec<ShapeId>,
    /// Textures resolved since the last load, by canonical path.
    tex_cache: HashMap<String, TexSlot>,
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
            shapes: BTreeMap::new(),
            draw_order: vec![],
            tex_cache: HashMap::new(),
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

    /// CS feature defines for the next load or rebuilt draw (default [`DEFAULT_FEATURES`]).
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

    /// Bounding sphere (centre, radius) of every shape drawn at load: `positions × transform` (skinned
    /// shapes: positions as-is, since the skinned VS ignores World and P0 bones are identity). Parent NiNode
    /// transforms are not applied in P1-A. Material edits do not move it.
    pub fn bounds(&self) -> Option<([f32; 3], f32)> {
        self.bounds
    }

    fn linear_lighting(&self) -> bool {
        self.features.iter().any(|f| f == "ENABLE_LL")
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

    /// The texture at canonical `norm`, resolved and decoded once per load. A path the resolver does not
    /// return is listed in `rep.missing_textures`; undecodable bytes are an `rep.errors` line.
    fn texture(
        &mut self,
        norm: &str,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> TexSlot {
        if let Some(t) = self.tex_cache.get(norm) {
            return t.clone();
        }
        let loaded = match textures(norm) {
            None => {
                if !rep.missing_textures.iter().any(|m| m == norm) {
                    rep.missing_textures.push(norm.to_string());
                }
                None
            }
            Some(bytes) => match textures::load_dds(&self.gpu, &bytes) {
                Ok(t) => Some((t.srv, t.kind)),
                Err(e) => {
                    rep.errors.push(format!("{norm}: {e}"));
                    None
                }
            },
        };
        self.tex_cache.insert(norm.to_string(), loaded.clone());
        loaded
    }

    /// NIF slot → register for every slot the PS reads (a texture is loaded only then), then a 1×1 default for
    /// every reflected register left without one. Returns `(register, view)` for every reflected SRV and the
    /// registers bound to a real texture.
    fn bind_textures(
        &mut self,
        name: &str,
        program: &Program,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(Vec<(u32, ID3D11ShaderResourceView)>, Bound), String> {
        let pbr = m.is_pbr();
        let msn = m.slsf1 & (1 << 12) != 0;
        let mut real: HashMap<u32, ID3D11ShaderResourceView> = HashMap::new();
        for (slot, path) in m.slots.iter().enumerate() {
            if path.norm.is_empty() {
                continue;
            }
            let Some(reg) = textures::register_for_slot(m.shader_type, pbr, msn, slot) else {
                continue;
            };
            let Some(kind) = draw::srv_kind_at(&program.ps.reflection, reg) else {
                continue;
            };
            match self.texture(&path.norm, textures, rep) {
                Some((srv, k)) if k == kind => {
                    real.insert(reg, srv);
                }
                Some(_) => rep.warnings.push(format!(
                    "{name}: {} is not a {kind:?} texture (t{reg}); default bound",
                    path.norm
                )),
                None => {}
            }
        }
        let mut srvs = vec![];
        for (_, reg, kind) in &program.ps.reflection.srvs {
            if let Some(v) = real.get(reg) {
                srvs.push((*reg, v.clone()));
                continue;
            }
            let rgba = textures::default_for(*reg, m.shader_type, pbr);
            let v = self
                .default_srv(*kind, rgba)
                .map_err(|e| format!("default texture: {e}"))?;
            srvs.push((*reg, v));
        }
        let bound = Bound {
            regs: real.keys().copied().collect(),
        };
        Ok((srvs, bound))
    }

    /// Builds one shape's draw for material `m`: program (compiled synchronously), textures, vertex streams
    /// against the VS inputs, zero cbuffers from the reflection, then the material.
    fn build_draw(
        &mut self,
        name: &str,
        mesh: &nif::geom::Mesh,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<ShapeDraw, BuildError> {
        let program = self
            .program(&perm_material(m))
            .map_err(BuildError::Compile)?;
        let (srvs, bound) = self
            .bind_textures(name, &program, m, textures, rep)
            .map_err(BuildError::Other)?;
        let gpu_mesh = mesh::build(
            &self.gpu,
            mesh,
            &program.vs.reflection.inputs,
            &program.vs.bytecode,
        )
        .map_err(BuildError::Other)?;
        let (mut vs_cbs, mut ps_cbs) = match (
            cbuffers(&self.gpu, &program.vs.reflection),
            cbuffers(&self.gpu, &program.ps.reflection),
        ) {
            (Ok(v), Ok(p)) => (v, p),
            (Err(e), _) | (_, Err(e)) => return Err(BuildError::Other(e)),
        };
        let ll = self.linear_lighting();
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
        }
        Ok(ShapeDraw {
            name: name.to_string(),
            vs: program.vs_obj.clone(),
            ps: program.ps_obj.clone(),
            mesh: gpu_mesh,
            vs_cbs,
            ps_cbs,
            srvs,
            samplers: program.ps.reflection.samplers.iter().map(|s| s.1).collect(),
            model: mesh.transform,
            blend: m.blend(),
            skinned: m.skinned,
            bound,
        })
    }

    /// Parses the NIF and loads it: `load_parsed(&parse(nif_bytes), textures)`. `textures` resolves a canonical
    /// texture path (`textures/…`, lower case, `/`) to DDS bytes.
    pub fn load(
        &mut self,
        nif_bytes: &[u8],
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_parsed(&parse(nif_bytes), textures)
    }

    /// Replaces the loaded model with `parsed`: one draw per shape (compiled synchronously), textures resolved
    /// through `textures`. A shape whose draw cannot be built stays listed with status `Failed`; a hidden
    /// material is listed as `Hidden` and not built.
    pub fn load_parsed(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.shapes.clear();
        self.draw_order.clear();
        self.tex_cache.clear();
        self.bounds = None;
        let mut rep = LoadReport {
            le_mesh: parsed.le_mesh,
            errors: parsed.errors.clone(),
            warnings: parsed.warnings.clone(),
            ..LoadReport::default()
        };
        if parsed.le_mesh {
            return rep;
        }
        for s in &parsed.shapes {
            let (draw, status) = if s.material.hidden {
                (None, ShapeStatus::Hidden)
            } else {
                match self.build_draw(&s.name, &s.mesh, &s.material, textures, &mut rep) {
                    Ok(d) => (Some(d), ShapeStatus::Drawing),
                    Err(BuildError::Compile(e)) => {
                        rep.compile_errors.push(format!("{}: {e}", s.name));
                        (None, ShapeStatus::Failed(e))
                    }
                    Err(BuildError::Other(e)) => {
                        rep.errors.push(format!("{}: {e}", s.name));
                        (None, ShapeStatus::Failed(e))
                    }
                }
            };
            self.shapes.insert(
                s.id,
                ShapeState {
                    name: s.name.clone(),
                    mesh: s.mesh.clone(),
                    material: s.material.clone(),
                    draw,
                    status,
                },
            );
        }
        self.bounds = self.compute_bounds();
        self.warn_missing_fields(&mut rep);
        self.sort_draw_order();
        rep.shapes = self
            .shapes
            .values()
            .filter(|s| s.status == ShapeStatus::Drawing)
            .count();
        rep
    }

    /// Replaces the loaded model with one built-in shape: `ShapeId(0)`, named after the shape. The
    /// geometry-bound fields are forced to match the mesh (no bones, no vertex colours).
    pub fn load_builtin(
        &mut self,
        b: Builtin,
        m: MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        let mut material = m;
        material.skinned = false;
        material.has_vertex_colors = false;
        let parsed = ParsedNif {
            shapes: vec![ParsedShape {
                id: ShapeId(0),
                name: format!("{b:?}"),
                mesh: Arc::new(builtin_mesh(b)),
                material,
            }],
            ..ParsedNif::default()
        };
        self.load_parsed(&parsed, textures)
    }

    /// Every loaded shape in id order, with the material last loaded or set and its status.
    pub fn shapes(&self) -> Vec<ShapeInfo> {
        self.shapes
            .iter()
            .map(|(id, s)| ShapeInfo {
                id: *id,
                name: s.name.clone(),
                material: s.material.clone(),
                status: s.status.clone(),
            })
            .collect()
    }

    /// Replaces shape `id`'s material and applies it the cheapest way [`classify`] allows (03 §1.3):
    /// - `Cbuffer`: every cbuffer of the draw back to zero, then the material; on the next frame, no compile;
    /// - `Textures`: the slots resolved again through `textures` and rebound, then the `Cbuffer` path (the PBR
    ///   flags and `EnvmapData.y` depend on what is bound);
    /// - `Permutation`: a new draw is built now (synchronous compile in P1-A; background compile is P1-B).
    ///
    /// A shape that is not `Drawing` (failed, hidden) is always rebuilt. On any build error the previous draw
    /// stays on screen and the status is `Failed(error)`; the result is still `Ok(kind)`. A hidden material
    /// stops drawing the shape and keeps its draw for later. Paths the resolver did not return before are asked
    /// for again. `Err` only for an id that is not loaded.
    pub fn set_material(
        &mut self,
        id: ShapeId,
        m: MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<EditKind, String> {
        let (kind, rebuild) = {
            let state = self
                .shapes
                .get(&id)
                .ok_or_else(|| format!("no shape with id {} is loaded", id.0))?;
            let features: Vec<&str> = self.features.iter().map(String::as_str).collect();
            let kind = classify(&state.material, &m, &features);
            let rebuild = state.draw.is_none()
                || state.status != ShapeStatus::Drawing
                || kind == EditKind::Permutation;
            (kind, rebuild)
        };
        if m.hidden {
            if let Some(state) = self.shapes.get_mut(&id) {
                state.material = m;
                state.status = ShapeStatus::Hidden;
            }
            self.sort_draw_order();
            return Ok(kind);
        }
        if kind == EditKind::Same && !rebuild {
            return Ok(kind);
        }
        self.tex_cache.retain(|_, t| t.is_some());
        let mut scratch = LoadReport::default();
        let outcome = if rebuild {
            let (name, mesh) = {
                let s = &self.shapes[&id];
                (s.name.clone(), s.mesh.clone())
            };
            match self.build_draw(&name, &mesh, &m, textures, &mut scratch) {
                Ok(d) => {
                    if let Some(s) = self.shapes.get_mut(&id) {
                        s.draw = Some(d);
                    }
                    Ok(())
                }
                Err(e) => Err(e.into_text()),
            }
        } else {
            self.apply_in_place(id, kind, &m, textures, &mut scratch)
        };
        if let Some(state) = self.shapes.get_mut(&id) {
            state.status = match outcome {
                Ok(()) => ShapeStatus::Drawing,
                Err(e) => ShapeStatus::Failed(e),
            };
            state.material = m;
        }
        self.sort_draw_order();
        Ok(kind)
    }

    /// A `Cbuffer` or `Textures` edit on the shape's existing draw (same permutation, so the program is an
    /// in-memory hit).
    fn apply_in_place(
        &mut self,
        id: ShapeId,
        kind: EditKind,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(), String> {
        if kind == EditKind::Textures {
            let name = self.shapes[&id].name.clone();
            let program = self.program(&perm_material(m))?;
            let (srvs, bound) = self.bind_textures(&name, &program, m, textures, rep)?;
            let d = self
                .shapes
                .get_mut(&id)
                .and_then(|s| s.draw.as_mut())
                .ok_or("no draw")?;
            d.srvs = srvs;
            d.bound = bound;
        }
        let ll = self.linear_lighting();
        let d = self
            .shapes
            .get_mut(&id)
            .and_then(|s| s.draw.as_mut())
            .ok_or("no draw")?;
        d.rewrite_material(m, ll);
        Ok(())
    }

    fn compute_bounds(&self) -> Option<([f32; 3], f32)> {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut points: Vec<[f32; 3]> = vec![];
        for s in self.shapes.values().filter(|s| s.draw.is_some()) {
            let t = &s.mesh.transform;
            for p in &s.mesh.positions {
                let q = if s.material.skinned {
                    *p
                } else {
                    [0, 1, 2].map(|r| t[r][0] * p[0] + t[r][1] * p[1] + t[r][2] * p[2] + t[r][3])
                };
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
                points.push(q);
            }
        }
        if points.is_empty() {
            return None;
        }
        let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let r = points
            .iter()
            .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        Some((c, r))
    }

    /// A required name no drawn shape's cbuffers have: logged once per load (CS version drift, 03 §6). The
    /// skinned VS reads neither World nor EyePosition (bones + BonesPivot instead), so those two are only
    /// expected from non-skinned shapes.
    fn warn_missing_fields(&self, rep: &mut LoadReport) {
        let draws: Vec<&ShapeDraw> = self
            .shapes
            .values()
            .filter_map(|s| s.draw.as_ref())
            .collect();
        if draws.is_empty() {
            return;
        }
        for n in scene::REQUIRED {
            let world_only = matches!(*n, "World" | "EyePosition");
            let candidates: Vec<&&ShapeDraw> = draws
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

    /// Opaque first, then alpha-blended (P0 does not sort blended shapes by depth); block order within each.
    /// Hidden shapes and shapes without a draw are left out.
    fn sort_draw_order(&mut self) {
        let mut order: Vec<(bool, ShapeId)> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.status != ShapeStatus::Hidden)
            .filter_map(|(id, s)| s.draw.as_ref().map(|d| (d.blend, *id)))
            .collect();
        order.sort_by_key(|(blend, _)| *blend);
        self.draw_order = order.into_iter().map(|(_, id)| id).collect();
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
            for id in &self.draw_order {
                let Some(s) = self.shapes.get_mut(id).and_then(|st| st.draw.as_mut()) else {
                    continue;
                };
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
