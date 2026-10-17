pub mod background;
pub mod builtin;
pub mod cache;
pub mod cbuffer;
pub mod compile;
pub mod cs_settings;
pub mod debug;
pub mod device;
pub mod draw;
pub mod hdri;
pub mod includes;
pub mod locate;
pub mod material;
pub mod mesh;
pub mod parse;
pub mod perf_log;
pub mod permutation;
pub mod pool;
pub mod scene;
pub mod source;
pub mod textures;

pub use builtin::{builtin_mesh, Builtin};
pub use debug::{Availability, DebugOutput};
pub use material::{classify, pbr_flags, Bound, EditKind};
pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};
pub use pool::{run_job, CompileDone, CompileJob, CompilePool};

use background::Backdrop;
use compile::{cached, program_key, CompiledPair, ProgramKey, SrvKind};
use cs_settings::CsSettings;
use device::{Gpu, Target};
use draw::{cbuffers, create_program, FrameInputs, Program, ShapeDraw};
use locate::Fingerprint;
use material::{perm_material, write_material, write_skin_flags};
use nif::material::MaterialState;
use permutation::{defines, Defines, Stage};
pub use scene::Background;
use scene::{LightValues, PointValues, Scene, View, ViewValues};
use source::{ExpandedSource, SourceSet};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    /// Shapes that are drawing or will draw (waiting for a program counts: an async first load of an uncached
    /// mesh has none drawing yet, and the GUI must still send a camera, 03 §1.4).
    pub shapes: usize,
    /// Of `shapes`, how many are waiting for a background compile.
    pub pending: usize,
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

/// What a loaded shape is doing on screen.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeStatus {
    /// The current material is on screen.
    Drawing,
    /// `MaterialState::hidden` (PGPatcher `delete`): not drawn.
    Hidden,
    /// Its shader program is compiling in the background. `showing_previous`: the draw of an earlier material
    /// is still on screen, with its old constants, until the program lands.
    Pending { showing_previous: bool },
    /// The current material could not be built (compile, mesh or GPU error). `showing_previous`: the draw of an
    /// earlier material stays on screen.
    Failed {
        error: String,
        showing_previous: bool,
    },
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
/// loaded or set, and what is on screen (possibly built from an earlier material, see [`ShapeStatus`]).
struct ShapeState {
    name: String,
    mesh: Arc<nif::geom::Mesh>,
    world: [[f32; 4]; 3],
    material: MaterialState,
    draw: Option<ShapeDraw>,
    status: ShapeStatus,
    /// The program a `Pending` shape waits for.
    wanted: Option<Wanted>,
    /// `Failed` because the program did not compile (a `LoadReport::compile_errors` line), not for a mesh or
    /// GPU error (an `errors` line).
    compile_failed: bool,
}

/// A permutation as the compile pool needs it: its pair key, the shared expanded source, both define sets.
struct Wanted {
    key: ProgramKey,
    src: Arc<ExpandedSource>,
    vs: Defines,
    ps: Defines,
    /// When the shape started waiting (the `shape_ready` line's `ms_pending`).
    since: std::time::Instant,
}

impl Wanted {
    fn job(&self, cache_dir: &Path) -> CompileJob {
        CompileJob {
            key: self.key.clone(),
            src: self.src.clone(),
            vs: self.vs.clone(),
            ps: self.ps.clone(),
            cache_dir: cache_dir.to_path_buf(),
        }
    }
}

#[allow(clippy::large_enum_variant)] // a transient return value, never stored
enum Lookup {
    Ready(Program),
    Failed(String),
    Waiting,
}

fn ready(r: &Result<Program, String>) -> Lookup {
    match r {
        Ok(p) => Lookup::Ready(p.clone()),
        Err(e) => Lookup::Failed(e.clone()),
    }
}

/// A texture resolved once per load: its view and kind, or `None` (the resolver had no bytes, or they did
/// not decode).
type TexSlot = Option<(ID3D11ShaderResourceView, SrvKind)>;

/// Which of a path's two textures the draws use: the one the host's resolver returned at load (or a `Source`
/// swap), or the converter's encode of it. The set preview maps `PreviewMode` to this one to one (P1-B A/B, 03 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Variant {
    #[default]
    Source,
    Encoded,
}

impl Variant {
    pub fn other(self) -> Variant {
        match self {
            Variant::Source => Variant::Encoded,
            Variant::Encoded => Variant::Source,
        }
    }
}

/// Both textures of one canonical path (`textures/…`) and the one the draws use. GPU textures are immutable, so a
/// swap makes a new texture and SRV and rebinds. Both `None` = the resolver had nothing (kept so one load asks once).
#[derive(Clone, Default)]
struct TexEntry {
    source: Option<textures::Texture>,
    encoded: Option<textures::Texture>,
    active: Variant,
    /// A texture-set slot path the resolver did not return (a `LoadReport::missing_textures` line); never set for an
    /// RFAOS guess. Kept through swaps, so a dropped swap falls back to the missing-texture fill.
    missing: bool,
}

impl TexEntry {
    fn slot(&self, v: Variant) -> &Option<textures::Texture> {
        match v {
            Variant::Source => &self.source,
            Variant::Encoded => &self.encoded,
        }
    }

    fn slot_mut(&mut self, v: Variant) -> &mut Option<textures::Texture> {
        match v {
            Variant::Source => &mut self.source,
            Variant::Encoded => &mut self.encoded,
        }
    }

    fn current(&self) -> Option<&textures::Texture> {
        self.slot(self.active).as_ref()
    }

    fn any(&self) -> bool {
        self.source.is_some() || self.encoded.is_some()
    }
}
/// [`Renderer::draw_diag`]: PS defines, PS reflection, (register, DXGI format) of every bound SRV.
pub type DrawDiag = (
    Vec<(String, String)>,
    Option<compile::Reflection>,
    Vec<(u32, i32)>,
);

/// The reflected names of a program, both stages ([`cs_settings::reflected_fields`]).
fn program_fields(p: &Program) -> BTreeSet<String> {
    cs_settings::reflected_fields(
        p.vs.reflection
            .cbuffers
            .iter()
            .chain(&p.ps.reflection.cbuffers),
    )
}

/// One draw's FeatureData values: the settings against that draw's program (P1-D: exceptions keyed on its fields).
fn values_for(
    s: &CsSettings,
    features_on: bool,
    head_part: bool,
    fields: &BTreeSet<String>,
    m: &MaterialState,
) -> Vec<(String, Vec<f64>)> {
    cs_settings::feature_data(s, fields, features_on, is_head(m, head_part))
}

/// CS's head (bodyTiling = 1): a geometry under a `BSFaceGenNiNode` (advanced-skin Renderer.cpp:133-134). That
/// node exists only at runtime, so (Note BA) a shape is head when its shader type is FaceGen (4; the old shader's
/// own head split, 1.8 Lighting.hlsl:2047) or its NIF is a head part (`head_part`: a facegendata NIF or an HDPT
/// model; the mouth of capture 11178 ev 4269 is type 5 with head tiling).
fn is_head(m: &MaterialState, head_part: bool) -> bool {
    head_part || m.shader_type == 4
}

/// [`textures::rfaos_candidates`] of a material (slot 1 normal, slot 7 specular).
fn rfaos_candidates(m: &MaterialState) -> Vec<String> {
    let norm = |i: usize| m.slots.get(i).map_or("", |p| p.norm.as_str());
    textures::rfaos_candidates(m.shader_type, norm(1), norm(7))
}

/// The 3D preview renderer: message-shaped (load bytes → report, render scene → pixels) so it can later move
/// into a sidecar process. Single-threaded, like [`Gpu`].
pub struct Renderer {
    gpu: Gpu,
    shaders: Fingerprint,
    cache_dir: PathBuf,
    /// The CS feature defines every program is compiled with. `set_features` replaces it whole; `set_settings`
    /// and `set_features_on` replace only the settings defines (`ENABLE_LL` and [`cs_settings::PREVIEW_FEATURES`]).
    features: Vec<String>,
    /// The user CS settings last applied (P1-C C11); default = no settings files found.
    settings: CsSettings,
    /// The "Community Shaders features" toggle (C8): off, the settings add no define.
    features_on: bool,
    /// The shaders are an old Linear Lighting build (P1-D); memoised from the expanded source, reset by `set_shaders`.
    old_ll: std::cell::OnceCell<bool>,
    /// The last coverage of every program at hand (P1-D), for the panel line; dropped with the programs.
    coverage: HashMap<ProgramKey, cs_settings::Coverage>,
    /// One expanded `Lighting.hlsl` per shaders folder (03 §5); dropped with the folder by `set_shaders`.
    sources: SourceSet,
    /// Device objects by pair key; dropped by `set_shaders` and by a device rebuild.
    programs: HashMap<ProgramKey, Result<Program, String>>,
    /// Plain-data results by pair key (a worker's answer or a disk hit). Content-addressed, so they survive
    /// `set_shaders` and a device rebuild: a rebuilt device compiles nothing.
    compiled: HashMap<ProgramKey, Result<Arc<CompiledPair>, String>>,
    /// Keys whose job was queued and has not been applied yet.
    inflight: HashSet<ProgramKey>,
    /// Queued, not yet taken by the host (`take_jobs`).
    jobs: Vec<CompileJob>,
    /// Hosts that drive a pool turn this on; the default settles every load and edit before returning.
    async_compile: bool,
    /// The loaded NIF is a head part (Note BA; [`ParsedNif::head_part`]), set by every load.
    head_part: bool,
    defaults: HashMap<(SrvKind, [u8; 4]), ID3D11ShaderResourceView>,
    /// Every Lighting shape of the loaded model, keyed by NIF block index (built-ins: `ShapeId(0)`).
    shapes: BTreeMap<ShapeId, ShapeState>,
    /// Shapes with a draw that are not hidden: opaque first, then alpha-blended, block order within each.
    draw_order: Vec<ShapeId>,
    /// Textures resolved or swapped in since the last load, by canonical path.
    tex_cache: HashMap<String, TexEntry>,
    /// Missing slot paths whose texture-less entry an edit evicted (so the resolver is asked again); a later
    /// `replace_texture` that recreates the entry keeps `TexEntry::missing` from here. Cleared per load.
    evicted_missing: HashSet<String>,
    /// The variant the resolver's bytes of the next loads count as ([`Renderer::set_load_variant`]).
    load_variant: Variant,
    bounds: Option<([f32; 3], f32)>,
    sampler: ID3D11SamplerState,
    raster: ID3D11RasterizerState,
    alpha_blend: ID3D11BlendState,
    target: Option<Target>,
    encode_srgb: bool,
    timing: FrameTiming,
    /// The GPU-idle event query, created once (03 §7).
    query: ID3D11Query,
    /// Scene and view of the last rendered frame: what changed decides which fields are written.
    last_frame: Option<(Scene, View)>,
    /// Cbuffer uploads of the last frame.
    uploads: usize,
    /// Test hook: draw with P0's by-name writer (every field, every cbuffer, every frame).
    reference_writer: bool,
    /// The HDRI background pass, made on the first `Background::Hdri` frame.
    backdrop: Option<Result<Backdrop, String>>,
    /// [`Renderer::set_missing_texture`]: the colour a missing colour-slot texture shows; `None` = neutral defaults.
    missing_fill: Option<[u8; 4]>,
    /// [`Renderer::override_slot`]: NIF slot → the DDS bytes (kept to re-upload on a new device) and the texture every
    /// shape shows there. Not cleared by a load.
    overrides: BTreeMap<usize, (Arc<[u8]>, textures::Texture)>,
    /// [`Renderer::set_debug_output`]: the term the next permutations write instead of the lit colour.
    debug: DebugOutput,
}

impl Renderer {
    /// Creates the device (hardware, else WARP; see [`Gpu::new`]) and the fixed pipeline states.
    pub fn new(shaders: Fingerprint, cache_dir: PathBuf) -> Result<Renderer, String> {
        cache::prune_in_background(cache_dir.clone());
        Renderer::with_gpu(Gpu::new()?, shaders, cache_dir)
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
        let mut q: Option<ID3D11Query> = None;
        let qd = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        // SAFETY: descriptor and out-pointer are locals.
        unsafe { gpu.device.CreateQuery(&qd, Some(&mut q)) }
            .map_err(|e| format!("CreateQuery: {e}"))?;
        let query = q.ok_or("no event query")?;
        Ok(Renderer {
            query,
            last_frame: None,
            uploads: 0,
            reference_writer: false,
            backdrop: None,
            missing_fill: None,
            overrides: BTreeMap::new(),
            debug: DebugOutput::Lit,
            gpu,
            sources: SourceSet::new(shaders.root.clone()),
            shaders,
            cache_dir,
            // = preview_defines(&CsSettings::default(), true): the P0 Linear Lighting default.
            features: DEFAULT_FEATURES.iter().map(|s| s.to_string()).collect(),
            settings: CsSettings::default(),
            features_on: true,
            old_ll: std::cell::OnceCell::new(),
            coverage: HashMap::new(),
            programs: HashMap::new(),
            compiled: HashMap::new(),
            inflight: HashSet::new(),
            jobs: vec![],
            async_compile: false,
            head_part: false,
            defaults: HashMap::new(),
            shapes: BTreeMap::new(),
            draw_order: vec![],
            tex_cache: HashMap::new(),
            evicted_missing: HashSet::new(),
            load_variant: Variant::Source,
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

    /// The whole CS feature define list for the next load or rebuilt draw (default [`DEFAULT_FEATURES`]): tests
    /// and P0 callers. A later [`Renderer::set_settings`] / [`Renderer::set_features_on`] replaces only the
    /// settings defines in it and keeps the rest (the stand-in STANDIN_* hooks) in order.
    pub fn set_features(&mut self, features: Vec<String>) {
        self.features = features;
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    /// Applies the user CS settings (P1-C C11). The FeatureData values are recomputed per draw. If the settings
    /// defines are unchanged, the static block of every drawing shape is rewritten (a cbuffer edit, next
    /// frame). If they changed, every shape goes through the permutation path, and the old draws keep drawing
    /// with their old constants until their programs land. The display sRGB encode follows `ENABLE_LL`.
    pub fn set_settings(&mut self, s: CsSettings) {
        self.settings = s;
        self.apply_settings_defines();
        self.refresh_coverage();
    }

    /// Re-reports every program at hand (settings or the features toggle changed what is written).
    fn refresh_coverage(&mut self) {
        let at_hand: Vec<(ProgramKey, BTreeSet<String>)> = self
            .programs
            .iter()
            .filter_map(|(k, p)| p.as_ref().ok().map(|p| (k.clone(), program_fields(p))))
            .collect();
        for (k, f) in at_hand {
            self.note_coverage(k, f);
        }
    }

    /// The union of every program's coverage (P1-D): the panel shows `line()` when it is `Some`.
    pub fn settings_coverage(&self) -> cs_settings::Coverage {
        let mut all = cs_settings::Coverage::default();
        for c in self.coverage.values() {
            all.unset.extend(c.unset.iter().cloned());
            all.unmatched.extend(c.unmatched.iter().cloned());
        }
        all
    }

    /// Logs and keeps one program's coverage (`settings_coverage` line). The coverage depends only on names, so
    /// a non-head draw stands for the program.
    fn note_coverage(&mut self, key: ProgramKey, fields: BTreeSet<String>) {
        let values = cs_settings::feature_data(&self.settings, &fields, self.features_on, false);
        let c = cs_settings::coverage(&self.settings, &fields, &values);
        perf_log::emit(
            "settings_coverage",
            serde_json::json!({"key": perf_log::short(&key.0), "unset": c.unset, "unmatched": c.unmatched}),
        );
        self.coverage.insert(key, c);
    }

    pub fn settings(&self) -> &CsSettings {
        &self.settings
    }

    /// The "Community Shaders features" toggle (C8): off drops every settings define (LL included) and the sRGB
    /// encode; applied like [`Renderer::set_settings`].
    pub fn set_features_on(&mut self, on: bool) {
        self.features_on = on;
        self.apply_settings_defines();
        self.refresh_coverage();
    }

    /// Features CS loaded (Info.ini, or the Features inis) that this preview does not compile, in name order, for
    /// the panel "off in preview" line. TruePBR is never listed: it has no define (TRUE_PBR is per material)
    /// and its settings are always written. Features in `NON_VISUAL` (no shader/visual effect in this preview) are never listed either.
    pub fn off_in_preview(&self) -> Vec<String> {
        self.settings
            .loaded
            .iter()
            .filter(|n| n.as_str() != "TruePBR" && !NON_VISUAL.contains(&n.as_str()))
            .filter(|n| !define_of(n).is_some_and(|d| self.features.iter().any(|f| f == d)))
            .cloned()
            .collect()
    }

    /// `set_settings` / `set_features_on`: the defines the settings do not manage, in their order, then
    /// `preview_defines`. Unchanged as a set (`program_key` sorts defines) -> the static block of every drawing
    /// shape is rewritten in place; changed -> every shape through [`Renderer::begin`].
    fn apply_settings_defines(&mut self) {
        let mut f: Vec<String> = self
            .features
            .iter()
            .filter(|d| !settings_define(d))
            .cloned()
            .collect();
        f.extend(cs_settings::preview_defines(
            &self.settings,
            self.features_on,
        ));
        let (mut old, mut new) = (self.features.clone(), f.clone());
        old.sort();
        new.sort();
        // Linear Lighting draws in linear working space: encode for display (was the GUI's apply_features).
        self.features = f;
        self.encode_srgb = self.linear_lighting();
        if old == new {
            self.rewrite_static();
        } else {
            self.begin_all();
        }
    }

    /// New FeatureData values on every `Drawing` shape: the cbuffer-edit path of `apply_in_place` with the
    /// shape's current material. Pending/failed shapes keep their old draw constants (P1-B); a pending one gets
    /// the new values when its program lands.
    fn rewrite_static(&mut self) {
        let ll = self.linear_lighting();
        for s in self.shapes.values_mut() {
            if s.status != ShapeStatus::Drawing {
                continue;
            }
            if let Some(d) = s.draw.as_mut() {
                let fv = values_for(
                    &self.settings,
                    self.features_on,
                    self.head_part,
                    &d.fields,
                    &s.material,
                );
                d.rewrite_material(&s.material, ll, &fv);
            }
        }
    }

    /// Every shape that is not hidden through [`Renderer::begin`] (a define-set change, C11). A program that is at
    /// hand swaps now; the others queue one job each and stay `Pending { showing_previous }` on their old draw.
    /// The resolver is never asked: every slot of a shape's material is in `tex_cache` since its load or edit.
    fn begin_all(&mut self) {
        let todo: Vec<(ShapeId, MaterialState)> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.status != ShapeStatus::Hidden)
            .map(|(id, s)| (*id, s.material.clone()))
            .collect();
        let mut scratch = LoadReport::default();
        for (id, m) in todo {
            self.begin(id, &m, &|_| None, &mut scratch);
        }
        self.sort_draw_order();
        if !self.async_compile {
            self.settle();
        }
    }

    /// Replaces the shaders (a moved or updated CS): the expansion memo and the device programs are dropped (`compiled` is content-addressed and stays valid). The loaded model still uses the old programs until the next [`Renderer::load`].
    pub fn set_shaders(&mut self, shaders: Fingerprint) {
        self.sources = SourceSet::new(shaders.root.clone());
        self.shaders = shaders;
        self.programs.clear();
        self.old_ll = std::cell::OnceCell::new();
        self.coverage.clear();
        // The LL generation may have changed, so the display encode (follows linear_lighting) must be re-derived.
        self.encode_srgb = self.linear_lighting();
    }

    pub fn shaders(&self) -> &Fingerprint {
        &self.shaders
    }

    #[doc(hidden)]
    pub fn encode_srgb(&self) -> bool {
        self.encode_srgb
    }

    /// sRGB-encode the frame on readback (off by default). Turn it on for display when the features include
    /// `ENABLE_LL`: Linear Lighting draws in linear working space; without it the output is already gamma-space.
    #[doc(hidden)]
    pub fn set_encode_srgb(&mut self, on: bool) {
        self.encode_srgb = on;
    }

    /// `Some(rgba)`: a slot path the resolver did not return shows `rgba` (1×1, 2D or cube as the register reads it) in
    /// colour registers only: t0, and t4 when it reflects as a cube (the env map). Data registers keep `default_for`.
    /// `Bound` is not changed (material flags stay those of a missing file). Applies from the next load or rebind.
    pub fn set_missing_texture(&mut self, fill: Option<[u8; 4]>) {
        self.missing_fill = fill;
    }

    /// The debug view for every shape (GUI suggestions item 1). Err, with nothing changed, when `d` is not Lit and this
    /// CS build has no usable anchor ([`Renderer::debug_supported`]). Otherwise stores `d` and re-permutes every shape
    /// (`begin_all`): the old draw stays on screen while programs compile. Never short-circuits on an unchanged value.
    pub fn set_debug_output(&mut self, d: DebugOutput) -> Result<(), String> {
        if d != DebugOutput::Lit {
            self.sources.get_debug(Path::new("Lighting.hlsl"))?;
        }
        self.debug = d;
        self.begin_all();
        Ok(())
    }

    pub fn debug_output(&self) -> DebugOutput {
        self.debug
    }

    /// The loaded CS `Lighting.hlsl` has the single `return psout;` and the names the debug block reads. After
    /// `set_shaders` to a build without them, `debug_output()` keeps the request but every shape compiles Lit.
    pub fn debug_supported(&self) -> bool {
        self.sources.get_debug(Path::new("Lighting.hlsl")).is_ok()
    }

    /// Why the Studio HDRI backdrop could not be made (kept, so it is tried once per renderer), if it could not.
    pub fn backdrop_error(&self) -> Option<&str> {
        self.backdrop.as_ref()?.as_ref().err().map(String::as_str)
    }

    /// Timing of the last successful [`Renderer::render`].
    pub fn last_timing(&self) -> FrameTiming {
        self.timing
    }

    /// Cbuffer uploads of the last [`Renderer::render`] (0 when neither the camera, the lights nor any material
    /// changed).
    pub fn last_uploads(&self) -> usize {
        self.uploads
    }

    /// Test hook: draw with P0's by-name writer (`Scene::write` into every cbuffer and every upload, every
    /// frame), the reference the resolved path must match pixel for pixel.
    #[doc(hidden)]
    pub fn set_reference_writer(&mut self, on: bool) {
        self.reference_writer = on;
    }

    /// Bounding sphere (centre, radius) of every shape drawn at load: `positions × world` (the parent NiNode
    /// chain × the shape's own transform; skinned shapes: positions as-is, since the skinned VS ignores World and
    /// P0 bones are identity). Material edits do not move it.
    pub fn bounds(&self) -> Option<([f32; 3], f32)> {
        self.bounds
    }

    /// Linear Lighting on in the preview: the `ENABLE_LL` define, or on an old-LL build the cbuffer flag that
    /// `feature_data` writes (that build never gets the define).
    fn linear_lighting(&self) -> bool {
        if self.old_ll() {
            cs_settings::linear_lighting_on(&self.settings, true, self.features_on)
        } else {
            self.features.iter().any(|f| f == "ENABLE_LL")
        }
    }

    /// The shaders name `isDirLightLinear` (1.6, 1.8, Nexus 1.9.1). Read from the expanded `Lighting.hlsl` because
    /// the define list is decided before any program (and its reflection) exists. An unreadable source is not
    /// memoised.
    fn old_ll(&self) -> bool {
        if let Some(v) = self.old_ll.get() {
            return *v;
        }
        match self.sources.get(Path::new("Lighting.hlsl")) {
            Ok(src) => *self
                .old_ll
                .get_or_init(|| src.text.contains("isDirLightLinear")),
            Err(_) => false,
        }
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

    /// The active texture at canonical `norm`, resolved and decoded once per load (stored as the `load_variant`
    /// copy). A path the resolver does not return is listed in `rep.missing_textures`; undecodable bytes are an
    /// `rep.errors` line. `linear`: an `_SRGB` file is uploaded as UNORM (a data map, [`textures::load_dds_as`]).
    fn texture(
        &mut self,
        norm: &str,
        linear: bool,
        report_missing: bool,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> TexSlot {
        if let Some(e) = self.tex_cache.get(norm) {
            return e.current().map(|t| (t.srv.clone(), t.kind));
        }
        let v = self.load_variant;
        self.evicted_missing.remove(norm);
        let mut entry = TexEntry {
            active: v,
            ..TexEntry::default()
        };
        match textures(norm) {
            None => {
                entry.missing = report_missing;
                if !rep.missing_textures.iter().any(|m| m == norm) {
                    rep.missing_textures.push(norm.to_string());
                }
            }
            Some(bytes) => match textures::load_dds_as(&self.gpu, &bytes, linear) {
                Ok(t) => *entry.slot_mut(v) = Some(t),
                Err(e) => rep.errors.push(format!("{norm}: {e}")),
            },
        }
        let slot = entry.current().map(|t| (t.srv.clone(), t.kind));
        self.tex_cache.insert(norm.to_string(), entry);
        slot
    }

    /// NIF slot → register for every slot the PS reads (taken from `tex_cache`, which `resolve_textures` filled),
    /// then a 1×1 default for every reflected register left without one. Returns `(register, view)` for every
    /// reflected SRV and the registers bound to a real texture.
    fn bind_textures(
        &mut self,
        name: &str,
        program: &Program,
        m: &MaterialState,
        rep: &mut LoadReport,
    ) -> Result<(Vec<(u32, ID3D11ShaderResourceView)>, Bound), String> {
        let pbr = m.is_pbr();
        let msn = m.slsf1 & (1 << 12) != 0;
        let mut real: HashMap<u32, ID3D11ShaderResourceView> = HashMap::new();
        let mut missing: HashSet<u32> = HashSet::new();
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
            let entry = self.tex_cache.get(&path.norm);
            match entry
                .and_then(TexEntry::current)
                .map(|t| (t.srv.clone(), t.kind))
            {
                Some((srv, k)) if k == kind => {
                    real.insert(reg, srv);
                }
                Some(_) => rep.warnings.push(format!(
                    "{name}: {} is not a {kind:?} texture (t{reg}); default bound",
                    path.norm
                )),
                None => {
                    if entry.is_some_and(|e| e.missing) {
                        missing.insert(reg);
                    }
                }
            }
        }
        // CS_SKIN's t71: the first RFAOS candidate in `tex_cache` with a texture (resolve_textures asked for them,
        // linear). A hot-swap does not rebind it: sdc never encodes or swaps an RFAOS file (not a set member), and
        // a reload picks up an external edit.
        if let Some(kind) = draw::srv_kind_at(&program.ps.reflection, 71) {
            let found = rfaos_candidates(m)
                .iter()
                .find_map(|c| self.tex_cache.get(c).and_then(TexEntry::current))
                .filter(|t| t.kind == kind)
                .map(|t| t.srv.clone());
            if let Some(srv) = found {
                real.insert(71, srv);
            }
        }
        for (slot, (_, t)) in &self.overrides {
            let Some(reg) = textures::register_for_slot(m.shader_type, pbr, msn, *slot) else {
                continue;
            };
            if let Some(kind) =
                draw::srv_kind_at(&program.ps.reflection, reg).filter(|k| *k != t.kind)
            {
                rep.warnings.push(format!(
                    "{name}: the slot {slot} override is kind {:?} but t{reg} reads {kind:?}; not shown",
                    t.kind
                ));
            }
        }
        let mut srvs = vec![];
        for (_, reg, kind) in &program.ps.reflection.srvs {
            if let Some(v) = self.override_at(m, *reg, *kind) {
                srvs.push((*reg, v));
                continue;
            }
            if let Some(v) = real.get(reg) {
                srvs.push((*reg, v.clone()));
                continue;
            }
            let rgba = match self.missing_fill {
                Some(fill) if missing.contains(reg) && colour_register(*reg, *kind) => fill,
                _ => textures::default_for(*reg, m.shader_type, pbr),
            };
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

    /// Resolves every slot of `m` that maps to a register through `textures` into `tex_cache` (once per path
    /// per load). Done at load/edit time because `apply_compiled` has no resolver.
    fn resolve_textures(
        &mut self,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) {
        let (pbr, msn) = (m.is_pbr(), m.slsf1 & (1 << 12) != 0);
        for (slot, path) in m.slots.iter().enumerate() {
            if !path.norm.is_empty()
                && textures::register_for_slot(m.shader_type, pbr, msn, slot).is_some()
            {
                self.texture(&path.norm, false, true, textures, rep);
            }
        }
        // Derived guesses: a candidate the resolver lacks is not a missing texture (decode errors still count).
        let mut quiet = LoadReport::default();
        for c in rfaos_candidates(m) {
            if self
                .texture(&c, true, false, textures, &mut quiet)
                .is_some()
            {
                break;
            }
        }
        rep.errors.append(&mut quiet.errors);
    }

    /// Builds one shape's draw for material `m` on `program`: textures (already in `tex_cache`), vertex streams
    /// against the VS inputs, zero cbuffers from the reflection, then the material.
    fn build_draw(
        &mut self,
        name: &str,
        mesh: &nif::geom::Mesh,
        world: &[[f32; 4]; 3],
        m: &MaterialState,
        program: &Program,
        rep: &mut LoadReport,
    ) -> Result<ShapeDraw, String> {
        let (srvs, bound) = self.bind_textures(name, program, m, rep)?;
        let gpu_mesh = mesh::build(
            &self.gpu,
            mesh,
            &program.vs.reflection.inputs,
            &program.vs.bytecode,
        )?;
        let (mut vs_cbs, mut ps_cbs) = match (
            cbuffers(&self.gpu, &program.vs.reflection),
            cbuffers(&self.gpu, &program.ps.reflection),
        ) {
            (Ok(v), Ok(p)) => (v, p),
            (Err(e), _) | (_, Err(e)) => return Err(e),
        };
        let ll = self.linear_lighting();
        let fields = program_fields(program);
        let fv = values_for(&self.settings, self.features_on, self.head_part, &fields, m);
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
            scene::write_static(&mut cb.writer, &fv);
            write_skin_flags(&mut cb.writer, m, &bound);
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
            model: *world,
            blend: m.blend(),
            skinned: m.skinned,
            bound,
            built: m.clone(),
            stale: true,
            fields,
        })
    }

    /// The program `m` needs: both stage keys over the shared expansion of `Lighting.hlsl`.
    fn wanted(&self, m: &MaterialState) -> Result<Wanted, String> {
        let pm = perm_material(m);
        let old_ll = self.old_ll();
        // Old LL: ENABLE_LL is the shader's own alias of the cbuffer flag (Color.hlsli:7), never a -D (P1-D).
        let f: Vec<&str> = self
            .features
            .iter()
            .map(String::as_str)
            .filter(|d| !(old_ll && *d == "ENABLE_LL"))
            .collect();
        let vs = defines(&pm, Stage::Vertex, &f, false);
        let mut ps = defines(&pm, Stage::Pixel, &f, false);
        let lighting = Path::new("Lighting.hlsl");
        // A debug view: the injected source for both stages (one extra VS compile per permutation, shared by the
        // five views) and SDC_DEBUG on the PS only. Without an anchor, Lit (no failure per shape).
        let debug = match self.debug.define() {
            Some(n) => self.sources.get_debug(lighting).ok().map(|s| (n, s)),
            None => None,
        };
        let src = match debug {
            Some((n, s)) => {
                ps.0.push(("SDC_DEBUG".into(), n.to_string()));
                if self.encode_srgb {
                    ps.0.push(("SDC_DEBUG_PRELINEAR".into(), "1".into()));
                }
                s
            }
            None => self.sources.get(lighting)?,
        };
        let key = ProgramKey::pair(
            &program_key(&src, "main", "vs_5_0", &vs.0),
            &program_key(&src, "main", "ps_5_0", &ps.0),
        );
        Ok(Wanted {
            key,
            src,
            vs,
            ps,
            since: std::time::Instant::now(),
        })
    }

    /// The program if it is at hand without compiling: device objects, a memoised result (made into device
    /// objects now), or a disk-cache hit for both stages (~1 ms). Otherwise `Waiting`.
    fn lookup(&mut self, w: &Wanted) -> Lookup {
        if let Some(p) = self.programs.get(&w.key) {
            return ready(p);
        }
        if !self.compiled.contains_key(&w.key) {
            let dir = &self.cache_dir;
            let t0 = std::time::Instant::now();
            if let (Some(vs), Some(ps)) = (
                cached(&w.src, &w.vs, "vs_5_0", dir),
                cached(&w.src, &w.ps, "ps_5_0", dir),
            ) {
                perf_log::emit(
                    "compile",
                    serde_json::json!({"key": perf_log::short(&w.key.0), "ms": perf_log::ms_since(t0), "cache": "hit", "ok": true}),
                );
                self.compiled
                    .insert(w.key.clone(), Ok(Arc::new(CompiledPair { vs, ps })));
            }
        }
        match self.compiled.get(&w.key) {
            None => Lookup::Waiting,
            Some(Err(e)) => Lookup::Failed(e.clone()),
            Some(Ok(pair)) => {
                let built = create_program(&self.gpu, pair.vs.clone(), pair.ps.clone());
                self.programs.insert(w.key.clone(), built.clone());
                if let Ok(p) = &built {
                    self.note_coverage(w.key.clone(), program_fields(p));
                }
                ready(&built)
            }
        }
    }

    fn fail(&mut self, id: ShapeId, error: String, compile: bool) {
        if let Some(s) = self.shapes.get_mut(&id) {
            s.status = ShapeStatus::Failed {
                error,
                showing_previous: s.draw.is_some(),
            };
            s.wanted = None;
            s.compile_failed = compile;
        }
    }

    /// Replaces shape `id`'s draw with one built for `m` on `p`, only on success (03 §1.3 `swap_program`):
    /// on error the old draw stays and the status becomes `Failed { showing_previous: true }`.
    fn swap(&mut self, id: ShapeId, p: &Program, m: &MaterialState, rep: &mut LoadReport) {
        let (name, mesh, world) = match self.shapes.get(&id) {
            Some(s) => (s.name.clone(), s.mesh.clone(), s.world),
            None => return,
        };
        match self.build_draw(&name, &mesh, &world, m, p, rep) {
            Ok(d) => {
                if let Some(s) = self.shapes.get_mut(&id) {
                    s.draw = Some(d);
                    s.status = ShapeStatus::Drawing;
                    s.wanted = None;
                    s.compile_failed = false;
                }
            }
            Err(e) => self.fail(id, e, false),
        }
    }

    /// Starts building shape `id` for `m` (already stored as its material): swaps now when the program is at
    /// hand, else queues one job (deduplicated) and marks the shape `Pending`. The old draw, if any, stays.
    fn begin(
        &mut self,
        id: ShapeId,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) {
        self.resolve_textures(m, textures, rep);
        let wanted = match self.wanted(m) {
            Ok(w) => w,
            Err(e) => return self.fail(id, e, true),
        };
        match self.lookup(&wanted) {
            Lookup::Ready(p) => self.swap(id, &p, m, rep),
            Lookup::Failed(e) => self.fail(id, e, true),
            Lookup::Waiting => {
                perf_log::emit(
                    "shape_pending",
                    serde_json::json!({
                        "shape": id.0,
                        "key": perf_log::short(&wanted.key.0),
                        "showing_previous": self.shapes.get(&id).is_some_and(|s| s.draw.is_some()),
                    }),
                );
                if self.inflight.insert(wanted.key.clone()) {
                    self.jobs.push(wanted.job(&self.cache_dir));
                }
                if let Some(s) = self.shapes.get_mut(&id) {
                    s.status = ShapeStatus::Pending {
                        showing_previous: s.draw.is_some(),
                    };
                    s.wanted = Some(wanted);
                    s.compile_failed = false;
                }
            }
        }
    }

    /// Hosts that run their own pool: the jobs queued by loads and edits since the last call.
    pub fn take_jobs(&mut self) -> Vec<CompileJob> {
        std::mem::take(&mut self.jobs)
    }

    /// A finished job. Always memoised under its key; applied to every shape that waits for that key (a late
    /// result for a shape that moved on only warms the memo). True when a shape's draw or status changed (the
    /// host re-renders and refreshes its shape list).
    pub fn apply_compiled(&mut self, done: CompileDone) -> bool {
        let CompileDone { key, result } = done;
        self.inflight.remove(&key);
        self.compiled.insert(key.clone(), result);
        let waiting: Vec<ShapeId> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.wanted.as_ref().is_some_and(|w| w.key == key))
            .map(|(id, _)| *id)
            .collect();
        for id in &waiting {
            let Some(w) = self.shapes.get_mut(id).and_then(|s| s.wanted.take()) else {
                continue;
            };
            let since = w.since;
            let m = self.shapes[id].material.clone();
            match self.lookup(&w) {
                Lookup::Ready(p) => self.swap(*id, &p, &m, &mut LoadReport::default()),
                Lookup::Failed(e) => self.fail(*id, e, true),
                Lookup::Waiting => {
                    self.fail(*id, "the program is missing after its compile".into(), true)
                }
            }
            perf_log::emit(
                "shape_ready",
                serde_json::json!({
                    "shape": id.0,
                    "ms_pending": perf_log::ms_since(since),
                    "ok": self.shapes.get(id).is_some_and(|s| s.status == ShapeStatus::Drawing),
                }),
            );
        }
        // A worker panic is not a property of the program: forget it so the next edit retries.
        if matches!(self.compiled.get(&key), Some(Err(e)) if e.starts_with(pool::PANIC_PREFIX)) {
            self.compiled.remove(&key);
        }
        if !waiting.is_empty() {
            self.sort_draw_order();
        }
        !waiting.is_empty()
    }

    /// Runs every queued job on this thread and applies it: the synchronous mode (tests, the Meshes view's
    /// `load`). A key the host's pool already took (in flight) is compiled here too, so no shape stays `Pending`.
    pub fn settle(&mut self) {
        loop {
            let mut jobs = self.take_jobs();
            if jobs.is_empty() {
                let mut seen = HashSet::new();
                jobs = self
                    .shapes
                    .values()
                    .filter_map(|s| s.wanted.as_ref())
                    .filter(|w| seen.insert(w.key.clone()))
                    .map(|w| w.job(&self.cache_dir))
                    .collect();
            }
            if jobs.is_empty() {
                break;
            }
            for j in jobs {
                self.apply_compiled(run_job(j));
            }
        }
    }

    /// Turns the pool-driven mode on or off (default off). On: `load_parsed`, `load_builtin` and
    /// `set_material` return with `Pending` shapes and jobs for `take_jobs`; off: they settle before returning.
    pub fn set_async_compile(&mut self, on: bool) {
        self.async_compile = on;
    }

    /// After a device loss: a fresh device (the caller makes it, WARP if need be) with the same shaders, cache
    /// dir, features, CS settings and toggle, sRGB mode and `compiled` results. Everything that lived on the old
    /// device is gone, including the loaded model: load it again, and every program comes from `compiled` with no compile.
    pub fn rebuild_device_on(&mut self, gpu: Gpu) -> Result<(), String> {
        let mut n = Renderer::with_gpu(gpu, self.shaders.clone(), self.cache_dir.clone())?;
        n.features = std::mem::take(&mut self.features);
        n.settings = std::mem::take(&mut self.settings);
        n.features_on = self.features_on;
        n.encode_srgb = self.encode_srgb;
        n.async_compile = self.async_compile;
        n.load_variant = self.load_variant;
        n.reference_writer = self.reference_writer;
        n.compiled = std::mem::take(&mut self.compiled);
        n.missing_fill = self.missing_fill;
        n.debug = self.debug;
        // Device objects do not move: upload each override again (it decoded before, so a failure is not expected).
        for (slot, (bytes, _)) in std::mem::take(&mut self.overrides) {
            match textures::load_dds(&n.gpu, &bytes) {
                Ok(t) => {
                    n.overrides.insert(slot, (bytes, t));
                }
                Err(e) => {
                    eprintln!("render3d: slot {slot} override dropped on the new device: {e}")
                }
            }
        }
        *self = n;
        Ok(())
    }

    /// Parses the NIF and loads it, then settles: `load_parsed` in sync mode whatever the renderer's mode.
    /// `textures` resolves a canonical texture path (`textures/…`, lower case, `/`) to DDS bytes.
    pub fn load(
        &mut self,
        nif_bytes: &[u8],
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_parsed_sync(&parse(nif_bytes), textures)
    }

    /// `load_parsed` in sync mode whatever the renderer's mode (`load` for an already parsed NIF).
    pub fn load_parsed_sync(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_inner(parsed, textures, true)
    }

    /// Replaces the loaded model with `parsed`: geometry and textures now, one program per distinct
    /// permutation. A program that is at hand (in memory or a disk-cache hit) is applied now; the others are
    /// queued (`take_jobs`) and their shapes are `Pending` with no draw. In sync mode (the default) the queue
    /// is run before returning, so no shape is `Pending` and `LoadReport::compile_errors` is filled.
    pub fn load_parsed(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_inner(parsed, textures, !self.async_compile)
    }

    fn load_inner(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        sync: bool,
    ) -> LoadReport {
        self.shapes.clear();
        self.draw_order.clear();
        self.tex_cache.clear();
        self.evicted_missing.clear();
        self.bounds = None;
        self.head_part = parsed.head_part;
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
            let hidden = s.material.hidden;
            self.shapes.insert(
                s.id,
                ShapeState {
                    name: s.name.clone(),
                    mesh: s.mesh.clone(),
                    world: s.world,
                    material: s.material.clone(),
                    draw: None,
                    status: if hidden {
                        ShapeStatus::Hidden
                    } else {
                        ShapeStatus::Pending {
                            showing_previous: false,
                        }
                    },
                    wanted: None,
                    compile_failed: false,
                },
            );
            if !hidden {
                self.begin(s.id, &s.material, textures, &mut rep);
            }
        }
        if sync {
            self.settle();
        }
        for s in self.shapes.values() {
            if let ShapeStatus::Failed { error, .. } = &s.status {
                let line = format!("{}: {error}", s.name);
                if s.compile_failed {
                    rep.compile_errors.push(line);
                } else {
                    rep.errors.push(line);
                }
            }
        }
        self.bounds = self.compute_bounds();
        self.warn_missing_fields(&mut rep);
        self.sort_draw_order();
        rep.pending = self
            .shapes
            .values()
            .filter(|s| matches!(s.status, ShapeStatus::Pending { .. }))
            .count();
        rep.shapes = rep.pending
            + self
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
        let mesh = builtin_mesh(b);
        let parsed = ParsedNif {
            shapes: vec![ParsedShape {
                id: ShapeId(0),
                name: format!("{b:?}"),
                world: mesh.transform,
                mesh: Arc::new(mesh),
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
    /// - `Permutation`: the new program at hand is swapped in now; otherwise a job is queued (`take_jobs`) and
    ///   the shape is `Pending { showing_previous: true }`. The old draw keeps drawing, with its old
    ///   constants, until the program lands. In sync mode the queue is run before returning.
    ///
    /// A shape that is not `Drawing` (pending, failed, hidden) is always rebuilt. On any build error the
    /// previous draw stays on screen and the status is `Failed { showing_previous: true }`; the result is still
    /// `Ok(kind)`. A hidden material stops drawing the shape and keeps its draw for later. Paths the resolver
    /// did not return before are asked for again. `Err` only for an id that is not loaded.
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
                // A program still compiling for it is memoised when it lands, not applied.
                state.wanted = None;
                // Not updated while hidden: whatever returns it to the draw order must rewrite its fields.
                if let Some(d) = state.draw.as_mut() {
                    d.stale = true;
                }
            }
            self.sort_draw_order();
            return Ok(kind);
        }
        if kind == EditKind::Same && !rebuild {
            return Ok(kind);
        }
        let evicted = &mut self.evicted_missing;
        // Only missing entries are asked again; a decode error stays (begin_all never asks, so it would turn missing).
        self.tex_cache.retain(|p, e| {
            let evict = e.missing && !e.any();
            if evict {
                evicted.insert(p.clone());
            }
            !evict
        });
        let mut scratch = LoadReport::default();
        if rebuild {
            if let Some(s) = self.shapes.get_mut(&id) {
                s.material = m.clone();
            }
            self.begin(id, &m, textures, &mut scratch);
        } else {
            let outcome = self.apply_in_place(id, kind, &m, textures, &mut scratch);
            if let Some(state) = self.shapes.get_mut(&id) {
                state.status = match outcome {
                    Ok(()) => ShapeStatus::Drawing,
                    Err(error) => ShapeStatus::Failed {
                        error,
                        showing_previous: state.draw.is_some(),
                    },
                };
                state.material = m;
            }
        }
        self.sort_draw_order();
        if !self.async_compile {
            self.settle();
        }
        Ok(kind)
    }

    /// A `Cbuffer` or `Textures` edit on the shape's existing draw (same permutation, so the program is at
    /// hand).
    fn apply_in_place(
        &mut self,
        id: ShapeId,
        kind: EditKind,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(), String> {
        if kind == EditKind::Textures {
            self.resolve_textures(m, textures, rep);
            let name = self.shapes[&id].name.clone();
            let w = self.wanted(m)?;
            let program = match self.lookup(&w) {
                Lookup::Ready(p) => p,
                Lookup::Failed(e) => return Err(e),
                // Sync mode (no pool) compiles inline, as the old `program()` did, e.g. after `set_shaders`.
                Lookup::Waiting if !self.async_compile => {
                    let d = run_job(w.job(&self.cache_dir));
                    self.compiled.insert(d.key, d.result);
                    match self.lookup(&w) {
                        Lookup::Ready(p) => p,
                        Lookup::Failed(e) => return Err(e),
                        Lookup::Waiting => return Err("the program is not compiled".into()),
                    }
                }
                Lookup::Waiting => return Err("the program is not compiled".into()),
            };
            let (srvs, bound) = self.bind_textures(&name, &program, m, rep)?;
            let d = self
                .shapes
                .get_mut(&id)
                .and_then(|s| s.draw.as_mut())
                .ok_or("no draw")?;
            d.srvs = srvs;
            d.built = m.clone();
            d.bound = bound;
        }
        let ll = self.linear_lighting();
        let d = self
            .shapes
            .get_mut(&id)
            .and_then(|s| s.draw.as_mut())
            .ok_or("no draw")?;
        let fv = values_for(
            &self.settings,
            self.features_on,
            self.head_part,
            &d.fields,
            m,
        );
        d.rewrite_material(m, ll, &fv);
        Ok(())
    }

    /// Which variant the resolver's bytes count as for the loads that follow (default `Source`). The set preview
    /// sets it to its current mode before a scene push, so the loaded textures are that mode's copy and the other
    /// mode's can be swapped in beside it.
    pub fn set_load_variant(&mut self, v: Variant) {
        self.load_variant = v;
    }

    /// Hot-swaps one texture (03 §2): a new immutable texture + SRV from `dds` is stored as variant `v` of `path`
    /// (empty `dds`: that copy is dropped). A variant that is the entry's only texture becomes the active one; a
    /// dropped active copy falls back to the other. When the bound texture changes, every shape still drawing (not hidden)
    /// that reads `path` gets the new view, and its material is written again where the set of real textures changed
    /// (`PBRFlags` HasEmissive/…, `EnvmapData.y`). No mesh, shader or cbuffer layout is touched.
    ///
    /// Returns the `(shape, register)` pairs rebound. `Err`, with nothing stored or changed, for a DDS that does
    /// not decode or a texture of another kind than a register reads (a cube into a 2D slot).
    ///
    /// A register under a slot override (`override_slot`) keeps showing the override; the entry and `Bound` still
    /// change, and the pair is not returned.
    pub fn replace_texture(
        &mut self,
        path: &str,
        dds: &[u8],
        v: Variant,
    ) -> Result<Vec<(ShapeId, u32)>, String> {
        let fresh = if dds.is_empty() {
            None
        } else {
            Some(textures::load_dds(&self.gpu, dds).map_err(|e| format!("{path}: {e}"))?)
        };
        let mut entry = self
            .tex_cache
            .get(path)
            .cloned()
            .unwrap_or_else(|| TexEntry {
                missing: self.evicted_missing.contains(path),
                ..TexEntry::default()
            });
        let was = entry.active;
        let dropped = fresh.is_none();
        *entry.slot_mut(v) = fresh;
        if !dropped && entry.slot(v.other()).is_none() {
            entry.active = v;
        } else if dropped && entry.active == v {
            entry.active = v.other();
        }
        if entry.active == v || entry.active != was {
            self.rebind(path, entry)
        } else {
            self.tex_cache.insert(path.to_string(), entry);
            Ok(vec![])
        }
    }

    /// Makes variant `v` of `path` the bound one (the A/B flip): a new view per bound register, no upload, no
    /// decode. False when `v` is already active, `path` has no `v` copy, no `Drawing` shape reads `path`, or the
    /// copy's kind does not fit (then nothing changes).
    pub fn set_texture_variant(&mut self, path: &str, v: Variant) -> bool {
        let Some(mut entry) = self.tex_cache.get(path).cloned() else {
            return false;
        };
        if entry.active == v || entry.slot(v).is_none() {
            return false;
        }
        entry.active = v;
        self.rebind(path, entry)
            .is_ok_and(|touched| !touched.is_empty())
    }

    /// Shows `dds` in NIF texture slot `slot` of every shape (through each shape's slot→register rule), now and for
    /// later loads, over both variants, even where the slot path is empty. `None` removes it and rebinds what a load
    /// would bind (the active texture, the missing fill or the default). `Bound` and the material are never changed.
    /// A register of another kind keeps its binding (a load lists it under `warnings`). Err, with nothing changed, for
    /// a DDS that does not decode. Returns the `(shape, register)` pairs rebound.
    pub fn override_slot(
        &mut self,
        slot: usize,
        dds: Option<&[u8]>,
    ) -> Result<Vec<(ShapeId, u32)>, String> {
        match dds {
            Some(b) => {
                let t = textures::load_dds(&self.gpu, b)
                    .map_err(|e| format!("slot {slot} override: {e}"))?;
                self.overrides.insert(slot, (Arc::from(b), t));
            }
            None => {
                if self.overrides.remove(&slot).is_none() {
                    return Ok(vec![]);
                }
            }
        }
        let mut touched = vec![];
        for (id, reg, kind) in self.slot_targets(slot) {
            let Some(built) = self.shapes[&id].draw.as_ref().map(|d| d.built.clone()) else {
                continue;
            };
            let srv = match self.overrides.get(&slot) {
                Some((_, t)) if t.kind == kind => t.srv.clone(),
                Some(_) => continue,
                None => self.plain_srv(&built, slot, reg, kind)?,
            };
            if let Some(e) = self
                .shapes
                .get_mut(&id)
                .and_then(|s| s.draw.as_mut())
                .and_then(|d| d.srvs.iter_mut().find(|(r, _)| *r == reg))
            {
                e.1 = srv;
                touched.push((id, reg));
            }
        }
        Ok(touched)
    }

    /// The override texture a draw built for `m` shows at `reg`, if an override's slot maps there and its kind is the
    /// register's.
    fn override_at(
        &self,
        m: &MaterialState,
        reg: u32,
        kind: SrvKind,
    ) -> Option<ID3D11ShaderResourceView> {
        let msn = m.slsf1 & (1 << 12) != 0;
        self.overrides
            .iter()
            .find(|(slot, (_, t))| {
                t.kind == kind
                    && textures::register_for_slot(m.shader_type, m.is_pbr(), msn, **slot)
                        == Some(reg)
            })
            .map(|(_, (_, t))| t.srv.clone())
    }

    /// `(shape, register, kind)` for every visible draw whose built material maps NIF slot `slot` to a register it
    /// binds, with or without a path: [`Renderer::texture_targets`] keyed by slot.
    fn slot_targets(&self, slot: usize) -> Vec<(ShapeId, u32, SrvKind)> {
        let mut out = vec![];
        for (id, s) in &self.shapes {
            let Some(d) = s.draw.as_ref().filter(|_| s.status != ShapeStatus::Hidden) else {
                continue;
            };
            let m = &d.built;
            let msn = m.slsf1 & (1 << 12) != 0;
            let Some(reg) = textures::register_for_slot(m.shader_type, m.is_pbr(), msn, slot)
            else {
                continue;
            };
            let Some(view) = d.srvs.iter().find(|(r, _)| *r == reg).map(|(_, v)| v) else {
                continue;
            };
            if let Some(kind) = textures::view_kind(view) {
                out.push((*id, reg, kind));
            }
        }
        out
    }

    /// What `bind_textures` binds at `reg` for slot `slot` of `m` without an override: the active texture of the
    /// slot's path, else the missing fill (colour registers of a missing path, cached or evicted) or the register's
    /// default.
    fn plain_srv(
        &mut self,
        m: &MaterialState,
        slot: usize,
        reg: u32,
        kind: SrvKind,
    ) -> Result<ID3D11ShaderResourceView, String> {
        let norm = m
            .slots
            .get(slot)
            .map(|p| p.norm.as_str())
            .filter(|n| !n.is_empty());
        let entry = norm.and_then(|n| self.tex_cache.get(n));
        if let Some(t) = entry.and_then(TexEntry::current).filter(|t| t.kind == kind) {
            return Ok(t.srv.clone());
        }
        // An edit evicts a texture-less missing entry; its provenance lives on in `evicted_missing`.
        let missing = match entry {
            Some(e) => e.missing,
            None => norm.is_some_and(|n| self.evicted_missing.contains(n)),
        };
        let rgba = match self.missing_fill {
            Some(fill) if missing && colour_register(reg, kind) => fill,
            _ => textures::default_for(reg, m.shader_type, m.is_pbr()),
        };
        self.default_srv(kind, rgba)
    }

    /// `(shape, register, kind)` for every register a visible shape with a draw reads `path` through (the same slot →
    /// register rule as the load). Hidden shapes are left alone (their next build reads the entry). A pending or failed shape
    /// still draws its previous `ShapeDraw`, so it is rebound too, but its cbuffers are kept (its `material` is the
    /// newer, unapplied one).
    fn texture_targets(&self, path: &str) -> Vec<(ShapeId, u32, SrvKind)> {
        let mut out: Vec<(ShapeId, u32, SrvKind)> = vec![];
        for (id, s) in &self.shapes {
            let Some(d) = s.draw.as_ref().filter(|_| s.status != ShapeStatus::Hidden) else {
                continue;
            };
            let m = &d.built;
            let msn = m.slsf1 & (1 << 12) != 0;
            for (slot, p) in m.slots.iter().enumerate() {
                if p.norm != path {
                    continue;
                }
                let Some(reg) = textures::register_for_slot(m.shader_type, m.is_pbr(), msn, slot)
                else {
                    continue;
                };
                let Some(view) = d.srvs.iter().find(|(r, _)| *r == reg).map(|(_, v)| v) else {
                    continue;
                };
                let Some(kind) = textures::view_kind(view) else {
                    continue;
                };
                if !out.iter().any(|(i, r, _)| i == id && *r == reg) {
                    out.push((*id, reg, kind));
                }
            }
        }
        out
    }

    /// Stores `entry` for `path` and points every draw that reads it at the entry's active texture (a 1×1 default
    /// when it has none). All kinds are checked before anything changes.
    fn rebind(&mut self, path: &str, entry: TexEntry) -> Result<Vec<(ShapeId, u32)>, String> {
        let mut plan = vec![];
        for (id, reg, kind) in self.texture_targets(path) {
            let (shader_type, pbr, name, under) = {
                let s = &self.shapes[&id];
                let b = s.draw.as_ref().map_or(&s.material, |d| &d.built);
                (
                    b.shader_type,
                    b.is_pbr(),
                    s.name.clone(),
                    self.override_at(b, reg, kind).is_some(),
                )
            };
            let real = match entry.current() {
                Some(t) if t.kind != kind => {
                    return Err(format!(
                        "{path} is a {:?} texture but {name} reads t{reg} as {kind:?}",
                        t.kind
                    ))
                }
                Some(t) => Some(t.srv.clone()),
                None => None,
            };
            let srv = match &real {
                Some(v) => v.clone(),
                None => {
                    let rgba = match self.missing_fill {
                        Some(fill) if entry.missing && colour_register(reg, kind) => fill,
                        _ => textures::default_for(reg, shader_type, pbr),
                    };
                    self.default_srv(kind, rgba)?
                }
            };
            plan.push((id, reg, srv, real.is_some(), under));
        }
        self.tex_cache.insert(path.to_string(), entry);
        let ll = self.linear_lighting();
        let mut touched = vec![];
        for (id, reg, srv, real, under) in plan {
            let Some(s) = self.shapes.get_mut(&id) else {
                continue;
            };
            let material = s.material.clone();
            let applied = s.status == ShapeStatus::Drawing;
            let Some(d) = s.draw.as_mut() else {
                continue;
            };
            if !under {
                if let Some(slot) = d.srvs.iter_mut().find(|(r, _)| *r == reg) {
                    slot.1 = srv;
                }
            }
            let was = d.bound.regs.contains(&reg);
            if real {
                d.bound.regs.insert(reg);
            } else {
                d.bound.regs.remove(&reg);
            }
            if was != real && applied {
                let fv = values_for(
                    &self.settings,
                    self.features_on,
                    self.head_part,
                    &d.fields,
                    &material,
                );
                d.rewrite_material(&material, ll, &fv);
            }
            if !under {
                touched.push((id, reg));
            }
        }
        Ok(touched)
    }

    /// Diagnostic hook: the PS defines the shape's material compiles with, the PS reflection of that program (if it
    /// is at hand), and the shape's draw as (register, DXGI format of the bound view) pairs.
    #[doc(hidden)]
    pub fn draw_diag(&mut self, id: ShapeId) -> Option<DrawDiag> {
        let m = self.shapes.get(&id)?.material.clone();
        let w = self.wanted(&m).ok()?;
        let reflection = match self.programs.get(&w.key) {
            Some(Ok(p)) => Some(p.ps.reflection.clone()),
            _ => None,
        };
        let srvs = self.shapes[&id].draw.as_ref().map_or(vec![], |d| {
            d.srvs
                .iter()
                .map(|(r, v)| {
                    let mut desc = Default::default();
                    // SAFETY: `desc` is a live local the call fills in; `v` is a live view.
                    unsafe { v.GetDesc(&mut desc) };
                    (*r, desc.Format.0)
                })
                .collect()
        });
        Some((w.ps.0, reflection, srvs))
    }

    /// Test hook: the floats of `var` as written in the shape's pixel-stage cbuffers (`None` without a draw or when
    /// the PS does not reflect `var`).
    #[doc(hidden)]
    pub fn draw_cb_floats(&self, id: ShapeId, var: &str) -> Option<Vec<f32>> {
        let d = self.shapes.get(&id)?.draw.as_ref()?;
        let cb = d.ps_cbs.iter().find(|c| c.writer.has(var))?;
        let f = cb.writer.resolve(var)?;
        let bytes = cb
            .writer
            .bytes()
            .get(f.off as usize..(f.off + f.size) as usize)?;
        Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        )
    }

    /// Test hook: the COM pointer of the shape's index buffer. A hot-swap must leave it alone (no mesh reload).
    #[doc(hidden)]
    pub fn draw_identity(&self, id: ShapeId) -> Option<usize> {
        use windows::core::Interface;
        self.shapes
            .get(&id)?
            .draw
            .as_ref()
            .map(|d| d.mesh.index.as_raw() as usize)
    }

    /// Test hook: the `(kind, rgba)` of the 1×1 default bound at `reg` of the shape's draw; `None` when a real
    /// texture (or an override) is bound there, or there is no draw.
    #[doc(hidden)]
    pub fn draw_default_at(&self, id: ShapeId, reg: u32) -> Option<(SrvKind, [u8; 4])> {
        use windows::core::Interface;
        let d = self.shapes.get(&id)?.draw.as_ref()?;
        let bound = d.srvs.iter().find(|(r, _)| *r == reg)?.1.as_raw();
        self.defaults
            .iter()
            .find(|(_, v)| v.as_raw() == bound)
            .map(|(k, _)| *k)
    }

    /// Test hook: the registers the shape's draw counts as real textures (`Bound`), sorted.
    #[doc(hidden)]
    pub fn draw_bound(&self, id: ShapeId) -> Option<Vec<u32>> {
        let d = self.shapes.get(&id)?.draw.as_ref()?;
        let mut v: Vec<u32> = d.bound.regs.iter().copied().collect();
        v.sort_unstable();
        Some(v)
    }

    fn compute_bounds(&self) -> Option<([f32; 3], f32)> {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut points: Vec<[f32; 3]> = vec![];
        for s in self
            .shapes
            .values()
            .filter(|s| s.draw.is_some() || matches!(s.status, ShapeStatus::Pending { .. }))
        {
            let t = &s.world;
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
        // Taken, so a failed frame leaves "unknown" and the next one writes everything.
        let (lights_changed, view_changed) = match self.last_frame.take() {
            Some((s, v)) => (s != *scene, v != view),
            None => (true, true),
        };
        // Once per frame, not per cbuffer (03 §7): the four inverses, the light rows and the SH.
        let view_values = ViewValues::new(&view);
        let light_values = LightValues::new(scene);
        let point_values = PointValues::new(scene, &view);
        let inputs = FrameInputs {
            view: &view,
            view_values: &view_values,
            lights: &light_values,
            point: &point_values,
            lights_changed,
            view_changed,
        };
        let mut uploads = 0;
        if scene.background == Background::Hdri && self.backdrop.is_none() {
            // A failure is kept, so it is neither recompiled nor reported on every frame: the model draws on black.
            let made = Backdrop::new(&self.gpu);
            if let Err(e) = &made {
                perf_log::emit("backdrop_failed", serde_json::json!({ "error": e }));
            }
            self.backdrop = Some(made);
        }
        let gpu = &self.gpu;
        let target = self.target.as_ref().ok_or("no target")?;
        let clear = match scene.background {
            // The target is read back as is when not sRGB-encoding: give it the display value then.
            Background::Solid(c) if self.encode_srgb => c,
            Background::Solid(c) => c.map(scene::linear_to_srgb),
            Background::Hdri => [0.0; 3],
        };
        target.clear(gpu, [clear[0], clear[1], clear[2], 1.0]);
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
            if let (Background::Hdri, Some(Ok(b))) = (scene.background, &self.backdrop) {
                b.draw(gpu, &view, !self.encode_srgb, scene.env_yaw);
            }
            for id in &self.draw_order {
                let Some(s) = self.shapes.get_mut(id).and_then(|st| st.draw.as_mut()) else {
                    continue;
                };
                uploads += if self.reference_writer {
                    {
                        // Test hook only (P0's reference writer): recomputed every frame.
                        let fv = values_for(
                            &self.settings,
                            self.features_on,
                            self.head_part,
                            &s.fields,
                            &s.built,
                        );
                        s.update_by_name(gpu, scene, &view, &fv)
                    }
                } else {
                    s.update(gpu, &inputs)
                };
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
        self.uploads = uploads;
        let draw_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        wait_idle(gpu, &self.query)?;
        let gpu_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let t2 = std::time::Instant::now();
        let px = target.readback_display(gpu, self.encode_srgb)?;
        // Only a fully successful frame may suppress the next one's writes.
        self.last_frame = Some((scene.clone(), view));
        self.timing = FrameTiming {
            draw_ms,
            gpu_ms,
            readback_ms: t2.elapsed().as_secs_f64() * 1000.0,
        };
        Ok(px)
    }
}

/// A define the CS settings manage (C8): `ENABLE_LL` or a [`cs_settings::PREVIEW_FEATURES`] define.
fn settings_define(d: &str) -> bool {
    d == "ENABLE_LL" || cs_settings::PREVIEW_FEATURES.iter().any(|(_, x)| *x == d)
}

/// Owner D3: the missing-texture fill goes to colour registers only, the base colour t0 and the env cube at t4.
/// PBR t4 (`m_p`) and FaceGen t4 (detail) reflect as 2D and stay neutral.
fn colour_register(reg: u32, kind: SrvKind) -> bool {
    reg == 0 || (reg == 4 && kind == SrvKind::Cube)
}

/// Not visual in the preview: tools, display, performance.
const NON_VISUAL: [&str; 14] = [
    "CSEditor",
    "Effects11",
    "NRD",
    "PerformanceOverlay",
    "RemoteControl",
    "RenderDoc",
    "SceneManager",
    "Screenshot",
    "Upscaling",
    "GrassCollision",
    "GrassOptimizations",
    "TerrainHelper",
    "SkySync",
    "HDRDisplay",
];

/// The define a loaded CS feature (Info.ini short name) compiles as, if any: Linear Lighting is setting-driven
/// (`ENABLE_LL`), the others come from [`cs_settings::PREVIEW_FEATURES`].
fn define_of(feature: &str) -> Option<&'static str> {
    if feature == "LinearLighting" {
        return Some("ENABLE_LL");
    }
    cs_settings::PREVIEW_FEATURES
        .iter()
        .find(|(n, _)| *n == feature)
        .map(|(_, d)| *d)
}

/// Flushes and spins on the renderer's event query until the GPU has finished every submitted command, so the
/// readback that follows measures the copy alone. A failing query (removed device) is an error with the device
/// prefix.
fn wait_idle(gpu: &Gpu, q: &ID3D11Query) -> Result<(), String> {
    // SAFETY: `done` is a live BOOL-sized local, the size passed matches it; the query belongs to `gpu.device`.
    unsafe {
        gpu.ctx.End(q);
        gpu.ctx.Flush();
        let start = std::time::Instant::now();
        loop {
            let mut done = windows::core::BOOL(0);
            gpu.ctx
                .GetData(
                    q,
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
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    }
}

#[cfg(test)]
mod head_tests {
    use super::*;

    /// Note BA: a type-5 head part (the capture-11178 mouth) is head only when its NIF is flagged a head part;
    /// FaceGen (4) is head either way.
    #[test]
    fn head_parts_and_facegen_are_head() {
        let mut m = MaterialState::builtin_default();
        m.shader_type = 5;
        assert!(is_head(&m, true));
        assert!(!is_head(&m, false));
        m.shader_type = 4;
        assert!(is_head(&m, false));
    }
}
