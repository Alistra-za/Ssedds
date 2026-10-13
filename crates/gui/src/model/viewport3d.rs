//! The 3D viewport of the Meshes view (plan B Task B8): the selected NIF drawn with the user's Community Shaders
//! `Lighting.hlsl` by `render3d` (D3D11), and the CS-folder locate flow (00 §6.4).
//!
//! The `Renderer` lives on one long-lived worker thread (D3D11's immediate context is single-threaded); this model
//! sends it commands and drains its replies (`WorkerMsg::Viewport3d`) on the UI thread. Locating CS hashes every
//! shader file, so it runs on a short-lived worker too. The model holds pixels, never egui textures: `ui::viewport3d`
//! uploads a frame when its id changes.
use crate::model::meshes::ScanTarget;
use crate::worker::{spawn_guarded, WorkerMsg};
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::cs_settings::{self, CsSettings};
use render3d::device::Gpu;
use render3d::locate::{self, Change, Fingerprint};
use render3d::perf_log::{self, FrameStats, GapWatch};
use render3d::scene::{Background, Camera, PointLight, Scene, AUTO_DISTANCE, AUTO_PITCH, AUTO_YAW};
use render3d::{
    CompileDone, CompilePool, EditKind, FrameTiming, LoadReport, ParsedNif, Renderer, ShapeId,
    ShapeInfo, ShapeStatus, Variant,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Panel text when no CS install is found (00 §6.4).
pub const NOT_FOUND: &str = "Community Shaders not found";
pub const CHOOSE_FOLDER: &str = "Choose folder…";
pub const USE_NEW: &str = "Use new";
pub const KEEP_OLD: &str = "Keep old";
/// The one-line notice for a hash-only change.
pub const UPDATED_NOTICE: &str = "Community Shaders updated — recompiling shaders…";
/// The CS-features toggle (on by default). Off compiles without CS feature defines (P0: no `ENABLE_LL`).
pub const FEATURES_LABEL: &str = "Community Shaders features";
/// The footer's off-in-preview line until the user's CS settings have been read (P0 wording).
pub const CUBEMAPS_FOOTER: &str = "Dynamic Cubemaps: off in preview";
/// Notice prefix when a CS settings file did not parse (CS may be writing it): the previous settings stay (C10).
pub const SETTINGS_KEPT: &str = "Community Shaders settings not read, keeping the previous ones";

/// `DynamicCubemaps` → `Dynamic Cubemaps`, `LODBlending` → `LOD Blending` (Info.ini short names as CS's menu
/// spells them).
fn spaced(name: &str) -> String {
    let c: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &ch) in c.iter().enumerate() {
        let after_lower = i > 0 && c[i - 1].is_lowercase();
        let acronym_end =
            i > 0 && c[i - 1].is_uppercase() && c.get(i + 1).is_some_and(|n| n.is_lowercase());
        if ch.is_uppercase() && (after_lower || acronym_end) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// The panel's off-in-preview line: the user's loaded CS features this preview does not compile
/// (`Renderer::off_in_preview`); `fallback` until the thread has reported them (no settings read yet).
pub fn off_line(off: &[String], fallback: &str) -> String {
    if off.is_empty() {
        return fallback.to_string();
    }
    let names: Vec<String> = off.iter().map(|n| spaced(n)).collect();
    format!("Off in this preview: {}", names.join(", "))
}
/// Accessible name of the drawn frame.
pub const FRAME_LABEL: &str = "3D preview";

/// The popup text for a moved or renamed CS install. A side shows its owning mod, else its folder.
pub fn moved_text(old: &Fingerprint, new: &Fingerprint) -> String {
    let name = |f: &Fingerprint| {
        f.owner_mod
            .clone()
            .unwrap_or_else(|| f.root.display().to_string())
    };
    format!(
        "Community Shaders moved/updated: {} → {}. Use the new location?",
        name(old),
        name(new)
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum V3State {
    NotFound,
    Ready,
    Rendering,
    Error(String),
}

/// Where a file's bytes are: a loose file, an entry of a BSA, or bytes made in memory (a set-preview encode).
#[derive(Clone)]
pub enum FileSource {
    Loose(PathBuf),
    Bsa {
        archive: PathBuf,
        norm: String,
    },
    /// Equal by `id` only: every new encode gets a new id, and comparing DDS blobs on each `set_content` would be
    /// costly (02 §2.2).
    Memory {
        id: u64,
        bytes: Arc<Vec<u8>>,
    },
    /// A guess (a skin RFAOS candidate): a read that fails is no error, the texture is just absent.
    Optional(Box<FileSource>),
}

impl PartialEq for FileSource {
    fn eq(&self, o: &FileSource) -> bool {
        match (self, o) {
            (FileSource::Loose(a), FileSource::Loose(b)) => a == b,
            (
                FileSource::Bsa {
                    archive: a,
                    norm: n,
                },
                FileSource::Bsa {
                    archive: b,
                    norm: m,
                },
            ) => a == b && n == m,
            (FileSource::Memory { id: a, .. }, FileSource::Memory { id: b, .. }) => a == b,
            (FileSource::Optional(a), FileSource::Optional(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Debug for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileSource::Loose(p) => f.debug_tuple("Loose").field(p).finish(),
            FileSource::Bsa { archive, norm } => f
                .debug_struct("Bsa")
                .field("archive", archive)
                .field("norm", norm)
                .finish(),
            FileSource::Memory { id, bytes } => f
                .debug_struct("Memory")
                .field("id", id)
                .field("len", &bytes.len())
                .finish(),
            FileSource::Optional(s) => f.debug_tuple("Optional").field(s).finish(),
        }
    }
}

impl FileSource {
    /// From a store location: (origin kind, origin root) and the canonical path.
    pub fn from_store(kind: i32, root: &str, norm: &str) -> FileSource {
        if kind == modscan::store::KIND_BSA {
            FileSource::Bsa {
                archive: root.into(),
                norm: norm.into(),
            }
        } else {
            FileSource::Loose(Path::new(root).join(norm.replace('/', "\\")))
        }
    }
}

/// What to draw: a NIF copy and the game's copy of each texture it names (canonical `textures/…` paths).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshSpec {
    /// The NIF copy id (anything unique per mesh).
    pub key: i64,
    pub nif: FileSource,
    pub textures: HashMap<String, FileSource>,
    /// BSVER 83: the panel shows the LE text and nothing is loaded or rendered (00 §6.5).
    pub le_mesh: bool,
}

/// What the panel draws: a NIF read through a [`MeshSpec`] (the Meshes view), a parsed NIF whose shape materials
/// the caller set (the set preview's simulated PGPatcher patch, P1-A), or a built-in shape.
#[derive(Debug, Clone)]
pub enum Content {
    Mesh(Arc<MeshSpec>),
    Parsed {
        /// Unique per (mesh, materials, textures): a new key is a new load.
        key: u64,
        nif: Arc<ParsedNif>,
        textures: Arc<HashMap<String, FileSource>>,
    },
    Builtin {
        key: u64,
        shape: Builtin,
        material: MaterialState,
        textures: Arc<HashMap<String, FileSource>>,
    },
}

impl PartialEq for Content {
    fn eq(&self, o: &Content) -> bool {
        match (self, o) {
            (Content::Mesh(a), Content::Mesh(b)) => a == b,
            (
                Content::Parsed {
                    key: a,
                    nif: n,
                    textures: t,
                },
                Content::Parsed {
                    key: b,
                    nif: m,
                    textures: u,
                },
            ) => a == b && Arc::ptr_eq(n, m) && t == u,
            (
                Content::Builtin {
                    key: a,
                    shape: s,
                    material: x,
                    textures: t,
                },
                Content::Builtin {
                    key: b,
                    shape: r,
                    material: y,
                    textures: u,
                },
            ) => a == b && s == r && x == y && t == u,
            _ => false,
        }
    }
}

impl Content {
    /// What a `load` log line calls this content.
    pub fn label(&self) -> String {
        match self {
            Content::Mesh(s) => format!("mesh:{}", s.key),
            Content::Parsed { key, .. } => format!("parsed:{key}"),
            Content::Builtin { shape, .. } => format!("builtin:{shape:?}"),
        }
    }

    /// BSVER 83: the panel shows the LE text and nothing is loaded (00 §6.5).
    pub fn le_mesh(&self) -> bool {
        match self {
            Content::Mesh(m) => m.le_mesh,
            Content::Parsed { nif, .. } => nif.le_mesh,
            Content::Builtin { .. } => false,
        }
    }

    fn load_cmd(&self, id: u64) -> V3Cmd {
        match self.clone() {
            Content::Mesh(spec) => V3Cmd::Load { id, spec },
            Content::Parsed { nif, textures, .. } => V3Cmd::LoadParsed { id, nif, textures },
            Content::Builtin {
                shape,
                material,
                textures,
                ..
            } => V3Cmd::LoadBuiltin {
                id,
                shape,
                material,
                textures,
            },
        }
    }

    fn textures(&self) -> &HashMap<String, FileSource> {
        match self {
            Content::Mesh(s) => &s.textures,
            Content::Parsed { textures, .. } | Content::Builtin { textures, .. } => textures,
        }
    }
}

/// A rendered frame: RGBA8, sRGB-encoded for display when Linear Lighting is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The render request it answers; the UI uploads again when it changes.
    pub id: u64,
}

/// Orbit camera relative to the model's bounds; the default is `Camera::Auto` (in front, a little above).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    /// Distance in multiples of the auto distance.
    pub zoom: f32,
}

impl Default for Orbit {
    fn default() -> Orbit {
        Orbit {
            yaw: AUTO_YAW,
            pitch: AUTO_PITCH,
            zoom: 1.0,
        }
    }
}

impl Orbit {
    pub fn camera(&self, bounds: Option<([f32; 3], f32)>) -> Camera {
        match bounds {
            None => Camera::Auto,
            Some((c, r)) => Camera::Orbit {
                target: c,
                yaw: self.yaw,
                pitch: self.pitch,
                distance: r.max(1e-3) * AUTO_DISTANCE * self.zoom,
            },
        }
    }
    /// A drag of (dx, dy) points: right turns the model to the right, down tilts the view down.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-1.55, 1.55);
    }
    /// Multiplies the distance by `factor` (> 1 is further away).
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.05, 20.0);
    }
    /// Double-click: frame the bounds again from the front.
    pub fn reset(&mut self) {
        *self = Orbit::default();
    }
}

/// The "View" controls of the panel: background and lights. Not saved; every start begins at the defaults (the
/// look the preview always had).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The studio HDRI behind the model (and as ambient light) instead of a flat grey.
    pub hdri: bool,
    /// The flat background grey in 0..=1, perceptual (sRGB): 0 = black.
    pub grey: f32,
    /// Multiplier on the directional light, 0..=4.
    pub sun: f32,
    /// Point light brightness, 0..=4; 0 is off.
    pub point: f32,
    /// Point light falloff radius in multiples of the model's bounding radius, [`Look::RADIUS_RANGE`].
    pub radius: f32,
}

impl Look {
    pub const SUN_RANGE: (f32, f32) = (0.0, 4.0);
    pub const POINT_RANGE: (f32, f32) = (0.0, 4.0);
    pub const RADIUS_RANGE: (f32, f32) = (0.1, 10.0);

    /// The scene for this look: `camera`, and the point light placed around `bounds` (none loaded: off).
    pub fn scene(&self, camera: Camera, bounds: Option<([f32; 3], f32)>) -> Scene {
        let mut s = Scene {
            camera,
            background: if self.hdri {
                Background::Hdri
            } else {
                Background::grey(self.grey)
            },
            sun_brightness: self.sun.clamp(Self::SUN_RANGE.0, Self::SUN_RANGE.1),
            ..Scene::neutral()
        };
        if let Some(b) = bounds {
            let radius = self
                .radius
                .clamp(Self::RADIUS_RANGE.0, Self::RADIUS_RANGE.1);
            s.point_light = PointLight::around(
                b,
                self.point.clamp(Self::POINT_RANGE.0, Self::POINT_RANGE.1),
                radius,
            );
        }
        s
    }
}

impl Default for Look {
    fn default() -> Look {
        Look {
            hdri: false,
            grey: 0.0,
            sun: 1.0,
            point: 0.0,
            radius: 2.0,
        }
    }
}

/// Commands to the render thread.
pub enum V3Cmd {
    SetShaders(Fingerprint),
    /// The "Community Shaders features" toggle (`Renderer::set_features_on`): no reload, changed shapes recompile.
    SetFeatures(bool),
    /// New user CS settings (`Renderer::set_settings`, C11): a cbuffer rewrite, recompiles only when the defines
    /// change, never a reload.
    SetSettings(CsSettings),
    Load {
        id: u64,
        spec: Arc<MeshSpec>,
    },
    /// A parsed NIF drawn with the materials in its shapes (the set preview).
    LoadParsed {
        id: u64,
        nif: Arc<ParsedNif>,
        textures: Arc<HashMap<String, FileSource>>,
    },
    LoadBuiltin {
        id: u64,
        shape: Builtin,
        material: MaterialState,
        textures: Arc<HashMap<String, FileSource>>,
    },
    /// One shape's new material on the loaded content (a JSON-only edit); applied after the load it follows and
    /// dropped by a later load in the same batch.
    SetMaterial {
        /// The load id of the content it was made for (replies echo it so stale ones are dropped).
        content: u64,
        id: ShapeId,
        material: MaterialState,
    },
    /// New DDS bytes for a texture path as one variant (`Renderer::replace_texture`); empty bytes drop that copy.
    /// Applied after the load it follows; a later load in the same batch drops it.
    SwapTexture {
        path: String,
        bytes: Arc<Vec<u8>>,
        variant: Variant,
    },
    /// Makes one variant of a path the bound one (`Renderer::set_texture_variant`): the instant A/B flip.
    SetTextureVariant {
        path: String,
        variant: Variant,
    },
    /// The variant the next loads' resolver bytes count as (`Renderer::set_load_variant`); sticky.
    SetLoadVariant(Variant),
    Render {
        id: u64,
        camera: Camera,
        w: u32,
        h: u32,
        look: Look,
    },
    /// A finished background compile, sent by the pool into the render loop's own queue.
    Compiled(CompileDone),
    /// The model is gone: end the loop (the pool's sink holds a sender to the loop's queue, so the channel no
    /// longer closes by itself).
    Quit,
}

/// Replies from the render thread and the locate worker.
#[derive(Debug)]
pub enum V3Reply {
    /// The renderer exists: adapter name, and whether it is WARP (software).
    Ready {
        adapter: String,
        warp: bool,
    },
    /// No renderer could be made (no D3D11 device at all).
    Failed(String),
    Loaded {
        id: u64,
        report: LoadReport,
        bounds: Option<([f32; 3], f32)>,
    },
    Frame {
        frame: Arc<Frame>,
        timing: FrameTiming,
    },
    /// A frame the render thread made on its own because a compile landed (same camera as the last `Render`).
    /// Its id comes from a private range, so the UI uploads it; it never counts as an answered `Render`.
    Refresh {
        frame: Arc<Frame>,
        timing: FrameTiming,
    },
    RenderError {
        id: u64,
        message: String,
    },
    /// The shapes of the loaded `Parsed`/`Builtin` content: after each load and after each batch of edits.
    Shapes(Vec<ShapeInfo>),
    /// One `SetMaterial` applied: its kind (`Cbuffer`/`Textures` compile nothing) and render-thread time, or why
    /// it failed (the old draw stays).
    MaterialSet {
        content: u64,
        id: ShapeId,
        result: Result<EditKind, String>,
        ms: f64,
    },
    /// One `SwapTexture` applied (or why it was not: the old texture stays).
    TextureSwap {
        content: u64,
        path: String,
        result: Result<(), String>,
    },
    /// A locate answer; `generation` tells a stale one (started before a later locate or "Choose folder…").
    /// `settings`: the user's CS settings read in the same worker, whenever a `Shaders` folder was found.
    Located {
        generation: u64,
        result: Result<Option<Fingerprint>, String>,
        settings: Option<CsSettings>,
    },
    Chosen {
        generation: u64,
        result: Result<Fingerprint, String>,
        settings: Option<CsSettings>,
    },
    /// `Renderer::off_in_preview()`, after the thread starts and after each settings or features change.
    OffInPreview(Vec<String>),
    /// `Renderer::settings_coverage().line()` whenever it changes (P1-D); `None` = nothing to report.
    Coverage(Option<String>),
}

/// `%LOCALAPPDATA%\sdc\shader-cache`.
pub fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("sdc").join("shader-cache"))
}

/// How long the viewport must be busy without a break before the spinner shows.
pub const SPINNER_DELAY: Duration = Duration::from_millis(300);

/// A command whose effect shows in a later frame (timing log): sent at `at`, answered by the first `Frame` with
/// an id above `after` (the last render id sent when the command was sent).
struct Mark {
    at: Instant,
    after: u64,
    what: MarkKind,
}

enum MarkKind {
    Edit {
        shape: u32,
        kind: Option<EditKind>,
    },
    Swap {
        path: String,
        variant: Variant,
        bytes: usize,
    },
    Flip {
        variant: Variant,
        paths: usize,
    },
}

pub struct Viewport3d {
    pub state: V3State,
    pub orbit: Orbit,
    /// Background and light controls; changing one re-renders (never reloads).
    pub look: Look,
    pub report: Option<LoadReport>,
    /// The moved/renamed question (`Change::Moved`).
    pub popup: Option<Change>,
    pub features_on: bool,
    /// The user's loaded CS features this preview leaves off (`V3Reply::OffInPreview`), for the panel line.
    pub off_in_preview: Vec<String>,
    /// The CS settings coverage line (`V3Reply::Coverage`), shown under the frame when `Some`.
    pub coverage: Option<String>,
    pub notice: Option<String>,
    /// The CS shaders in use (the `Shaders` folder), as saved in `gui.toml`.
    pub shaders: Option<Fingerprint>,
    /// The last frame; kept while CS moves or disappears until the user answers.
    pub frame: Option<Arc<Frame>>,
    pub timing: Option<FrameTiming>,
    /// CPU time of the last upload into egui (set by the UI).
    pub upload_ms: Option<f64>,
    /// (adapter, software renderer).
    pub adapter: Option<(String, bool)>,
    /// Which panel owns this viewport, for the `ui_gap` line.
    pub panel: &'static str,
    pub render_error: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub bounds: Option<([f32; 3], f32)>,
    /// Shapes of the loaded `Parsed`/`Builtin` content (empty for a `Mesh`).
    pub shapes: Vec<ShapeInfo>,
    /// The last material edit: its kind and render-thread time in ms.
    pub last_edit: Option<(EditKind, f64)>,
    /// Texture swaps the render thread refused (a bad DDS, a kind the slot does not read); the set preview lists them.
    pub swap_errors: Vec<String>,
    /// The variant the next load's resolver bytes count as, and the one the render thread was last told.
    load_variant: Variant,
    load_variant_sent: Variant,
    content: Option<Content>,
    need_load: bool,
    /// The pending load is a reload of the same model (`set_content(.., true)`): a compile failure keeps the frame.
    kept_view: bool,
    /// A reload left shapes with no draw (failed, or still compiling): the previous image stays and no render goes
    /// out until they have resolved (a render would blank or half-fill it).
    held_frame: bool,
    loaded: Option<u64>,
    pending_load: Option<u64>,
    last_sent: Option<(Camera, u32, u32, u64, Look)>,
    render_sent: u64,
    render_done: u64,
    edits_sent: u64,
    edits_done: u64,
    next_id: u64,
    locating: bool,
    /// Since when `busy()` has held without a break (for `show_spinner`).
    busy_since: Option<Instant>,
    locate_gen: u64,
    /// The last scan target `locate` was given, so "Choose folder…" reads settings through the same target.
    scan_target: Option<ScanTarget>,
    declined: Option<PathBuf>,
    shaders_changed: bool,
    /// This session's locate (or the user) has confirmed `shaders`: until then the saved fingerprint may be stale,
    /// and compiling with it would cache new-source bytecode under the old hash.
    confirmed: bool,
    /// The CS settings last applied: sent to the render thread, or given to it when it starts.
    settings: CsSettings,
    /// Hash of the malformed-settings failure already shown: a dismissed notice stays gone while the file is unchanged.
    failed_hash: Option<String>,
    /// The new location's settings, held while a Moved popup is open and applied by "Use new".
    popup_settings: Option<CsSettings>,
    thread: Option<Sender<V3Cmd>>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    gap: GapWatch,
    marks: Vec<Mark>,
    load_at: Option<Instant>,
}

impl Viewport3d {
    /// `saved` is `[render3d] shaders` from `gui.toml`.
    pub fn new(saved: Option<Fingerprint>) -> Viewport3d {
        let (tx, rx) = channel();
        Viewport3d {
            state: V3State::Ready,
            orbit: Orbit::default(),
            look: Look::default(),
            report: None,
            popup: None,
            features_on: true,
            off_in_preview: vec![],
            coverage: None,
            notice: None,
            shaders: saved,
            frame: None,
            timing: None,
            upload_ms: None,
            adapter: None,
            panel: "meshes",
            render_error: None,
            cache_dir: default_cache_dir(),
            bounds: None,
            content: None,
            shapes: vec![],
            last_edit: None,
            swap_errors: vec![],
            load_variant: Variant::Source,
            load_variant_sent: Variant::Source,
            edits_sent: 0,
            edits_done: 0,
            need_load: false,
            kept_view: false,
            held_frame: false,
            loaded: None,
            pending_load: None,
            last_sent: None,
            render_sent: 0,
            render_done: 0,
            next_id: 0,
            locating: false,
            busy_since: None,
            locate_gen: 0,
            scan_target: None,
            declined: None,
            shaders_changed: false,
            confirmed: false,
            settings: CsSettings::default(),
            failed_hash: None,
            popup_settings: None,
            thread: None,
            tx,
            rx,
            gap: GapWatch::default(),
            marks: vec![],
            load_at: None,
        }
    }

    /// For UI tests: nothing saved, the given state.
    pub fn with_state(state: V3State) -> Viewport3d {
        let mut v = Viewport3d::new(None);
        v.state = state;
        v
    }

    /// The generation of the latest locate or "Choose folder…" (older answers are ignored).
    pub fn locate_generation(&self) -> u64 {
        self.locate_gen
    }

    /// The CS settings last applied.
    pub fn settings(&self) -> &CsSettings {
        &self.settings
    }

    /// Test seam: stands in for the render thread; the returned receiver gets the commands.
    #[doc(hidden)]
    pub fn attach_test_thread(&mut self) -> Receiver<V3Cmd> {
        let (t, r) = channel();
        self.thread = Some(t);
        r
    }

    /// Test seam: the channel the render thread replies on (`poll` drains it).
    #[doc(hidden)]
    pub fn reply_sender_for_test(&self) -> Sender<WorkerMsg> {
        self.tx.clone()
    }

    #[doc(hidden)]
    pub fn render_sent_for_test(&self) -> u64 {
        self.render_sent
    }

    #[doc(hidden)]
    pub fn loaded_for_test(&self) -> Option<u64> {
        self.loaded
    }

    /// Whether this session has confirmed the shaders (a locate answer, "Use new", "Keep old" or a chosen folder);
    /// no render thread starts and nothing is sent before that.
    pub fn shaders_confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn locating(&self) -> bool {
        self.locating
    }

    /// A load, a render, a material edit, a locate or a background compile is in flight.
    pub fn busy(&self) -> bool {
        self.locating
            || self.pending_load.is_some()
            || self.render_done < self.render_sent
            || self.edits_done < self.edits_sent
            || self.compiling() > 0
    }

    /// Shapes waiting for a shader program: the latest `Shapes` reply, or the load report until the first one.
    /// Zero when the render thread is gone, so a dead worker cannot keep the panel busy.
    pub fn compiling(&self) -> usize {
        if self.thread.is_none() {
            return 0;
        }
        if self.shapes.is_empty() {
            return self.report.as_ref().map_or(0, |r| r.pending);
        }
        self.shapes
            .iter()
            .filter(|s| matches!(s.status, ShapeStatus::Pending { .. }))
            .count()
    }

    /// Whether to draw the busy spinner at `now`: only after `SPINNER_DELAY` of continuous work, so an ordinary frame
    /// render does not flash it. Call once per frame; the repaints `busy()` already asks for keep it ticking.
    pub fn show_spinner(&mut self, now: Instant) -> bool {
        if !self.busy() {
            self.busy_since = None;
            return false;
        }
        let since = *self.busy_since.get_or_insert(now);
        now.duration_since(since) >= SPINNER_DELAY
    }

    /// The fingerprint to save in `gui.toml`, when it changed since the last call.
    pub fn take_shaders_changed(&mut self) -> Option<Fingerprint> {
        std::mem::take(&mut self.shaders_changed)
            .then(|| self.shaders.clone())
            .flatten()
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Look for CS (on start and on window focus): the target's mods first, then the saved folder. Off the UI thread.
    pub fn locate(&mut self, target: Option<ScanTarget>) {
        if self.locating {
            return;
        }
        self.scan_target.clone_from(&target);
        self.locating = true;
        self.locate_gen += 1;
        let generation = self.locate_gen;
        let saved = self.shaders.clone();
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = locate_shaders(target.as_ref(), saved.as_ref());
            // Small files (~60 KB): a full re-read and hash on every locate (03 §3.4), same generation.
            let settings = match &result {
                Ok(Some(f)) => Some(read_cs_settings(target.as_ref(), f)),
                _ => None,
            };
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Located {
                generation,
                result,
                settings,
            }));
        });
    }

    /// "Choose folder…": a CS mod folder or its `Shaders` folder.
    pub fn choose_folder(&mut self, folder: PathBuf) {
        self.locating = true;
        self.locate_gen += 1;
        let generation = self.locate_gen;
        let target = self.scan_target.clone();
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = chosen_root(&folder)
                .and_then(|root| locate::fingerprint(&root, None).map_err(|e| e.to_string()));
            let settings = result
                .as_ref()
                .ok()
                .map(|f| read_cs_settings(target.as_ref(), f));
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Chosen {
                generation,
                result,
                settings,
            }));
        });
    }

    /// A locate result: same → nothing; hash only → notice and recompile; first find → adopt; moved → popup
    /// (unless this new location was declined this session); none → the panel says so (the last frame stays).
    pub fn on_located(&mut self, r: Result<Option<Fingerprint>, String>) {
        self.locating = false;
        let found = match r {
            Ok(f) => f,
            Err(e) => {
                // An IO error (MO2 copying files, say) is not "not found": keep the state, say why, retry on focus.
                self.notice = Some(format!("Looking for Community Shaders failed: {e}"));
                return;
            }
        };
        match locate::compare(self.shaders.as_ref(), found.clone()) {
            Change::Same => {
                self.confirmed = true;
                if self.state == V3State::NotFound {
                    self.state = V3State::Ready;
                }
            }
            Change::Updated => {
                self.notice = Some(UPDATED_NOTICE.into());
                if let Some(f) = found {
                    self.adopt(f);
                }
            }
            Change::Moved { new, .. } if self.shaders.is_none() => self.adopt(new),
            Change::Moved { old, new } => {
                if self.declined.as_ref() != Some(&new.root) {
                    self.popup = Some(Change::Moved { old, new });
                }
            }
            Change::NotFound { .. } => self.state = V3State::NotFound,
        }
    }

    /// The CS settings read with a locate answer (C10). If a file did not parse, the previous settings stay and
    /// the notice says why. A new hash goes to the render thread; with no thread yet, it starts with these.
    pub fn on_settings(&mut self, s: CsSettings) {
        if s.parse_failed {
            if self.failed_hash.as_ref() == Some(&s.hash) {
                return;
            }
            self.failed_hash = Some(s.hash.clone());
            self.notice = Some(format!("{SETTINGS_KEPT}: {}", s.notes.join("; ")));
            return;
        }
        self.failed_hash = None;
        if self
            .notice
            .as_deref()
            .is_some_and(|n| n.starts_with(SETTINGS_KEPT))
        {
            self.notice = None;
        }
        if s.hash == self.settings.hash {
            return;
        }
        self.settings = s.clone();
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetSettings(s));
        }
    }

    fn on_chosen(&mut self, r: Result<Fingerprint, String>) {
        self.locating = false;
        match r {
            Ok(f) => {
                self.declined = None;
                self.adopt(f);
            }
            Err(e) => self.notice = Some(e),
        }
    }

    /// "Use new" in the popup.
    pub fn use_new(&mut self) {
        if let Some(Change::Moved { new, .. }) = self.popup.take() {
            let s = self.popup_settings.take();
            self.adopt(new);
            if let Some(s) = s {
                self.on_settings(s);
            }
        }
    }

    /// "Keep old": keep drawing with the old folder and don't ask about this new one again this session.
    pub fn keep_old(&mut self) {
        if let Some(Change::Moved { new, .. }) = self.popup.take() {
            self.declined = Some(new.root);
            self.popup_settings = None;
            // The user accepts the saved fingerprint (Keep old is offered only while the old folder exists).
            self.confirmed = true;
        }
    }

    fn adopt(&mut self, f: Fingerprint) {
        self.popup = None;
        self.popup_settings = None;
        self.confirmed = true;
        self.shaders = Some(f.clone());
        self.shaders_changed = true;
        if matches!(self.state, V3State::NotFound) {
            self.state = V3State::Ready;
        }
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetShaders(f));
            self.reload_pending();
        }
    }

    /// The thread reloads the current mesh after a shaders or features change.
    fn reload_pending(&mut self) {
        if self.pending_load.is_none() {
            self.pending_load = self.loaded;
        }
        self.last_sent = None;
    }

    /// The "Community Shaders features" toggle (C8). The render thread recomputes the defines. Shapes whose
    /// permutation changes recompile in the background while their old draw stays. Nothing reloads.
    pub fn set_features(&mut self, on: bool) {
        if self.features_on == on {
            return;
        }
        self.features_on = on;
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetFeatures(on));
        }
    }

    /// The mesh the Meshes view shows (`None`: nothing selected). Loaded on the next `request_frame`.
    pub fn set_mesh(&mut self, spec: Option<MeshSpec>) {
        let same = match (&self.content, &spec) {
            (Some(Content::Mesh(a)), Some(b)) => **a == *b,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.set_content(spec.map(|s| Content::Mesh(Arc::new(s))), false);
        }
    }

    /// What to draw (`None`: nothing), loaded on the next `request_frame`. `keep_view`: the same model with new
    /// data (re-encoded textures, P1-A): the camera, the bounds and the last frame stay until the reload lands.
    pub fn set_content(&mut self, c: Option<Content>, keep_view: bool) {
        if self.content == c {
            return;
        }
        self.content = c;
        let le = self.content.as_ref().is_some_and(Content::le_mesh);
        self.need_load = self.content.is_some() && !le;
        self.loaded = None;
        self.pending_load = None;
        // Renders and edits in flight are for the old content; the thread drops those queued behind a new load
        // without a reply.
        self.render_done = self.render_sent;
        self.edits_done = self.edits_sent;
        self.shapes.clear();
        self.render_error = None;
        self.last_sent = None;
        self.kept_view = keep_view && !le;
        self.held_frame = false;
        if self.kept_view {
            return;
        }
        self.report = le.then(|| LoadReport {
            le_mesh: true,
            ..Default::default()
        });
        self.bounds = None;
        self.frame = None;
        self.orbit.reset();
    }

    /// A JSON-only edit (spec §2.3): one shape's material on the current `Parsed`/`Builtin` content, applied by the
    /// thread after the load it follows. False when no load of this content has been sent yet (the caller then puts
    /// the material into a new content instead).
    pub fn set_material(&mut self, id: ShapeId, material: MaterialState) -> bool {
        if !matches!(
            self.content,
            Some(Content::Parsed { .. } | Content::Builtin { .. })
        ) {
            return false;
        }
        if self.need_load || (self.loaded.is_none() && self.pending_load.is_none()) {
            return false;
        }
        let Some(t) = &self.thread else {
            return false;
        };
        let content = self.loaded.or(self.pending_load).expect("checked above");
        let cmd = V3Cmd::SetMaterial {
            content,
            id,
            material,
        };
        if t.send(cmd).is_err() {
            return false;
        }
        self.edits_sent += 1;
        self.mark(MarkKind::Edit {
            shape: id.0,
            kind: None,
        });
        // The next request_frame draws the edit.
        self.last_sent = None;
        true
    }

    /// The render thread, once a load of the current `Parsed`/`Builtin` content has been sent: the guard of every
    /// edit that follows a load (texture swaps).
    fn content_thread(&self) -> Option<Sender<V3Cmd>> {
        if !matches!(
            self.content,
            Some(Content::Parsed { .. } | Content::Builtin { .. })
        ) {
            return None;
        }
        if self.need_load || (self.loaded.is_none() && self.pending_load.is_none()) {
            return None;
        }
        self.thread.clone()
    }

    /// New DDS bytes for `path` as `variant` on the current content (empty bytes drop that copy), applied by the
    /// thread after the load it follows. False when no load of this content has been sent yet: the caller then puts
    /// the bytes into a new content instead.
    pub fn swap_texture(&mut self, path: String, bytes: Arc<Vec<u8>>, variant: Variant) -> bool {
        self.send_texture_cmd(V3Cmd::SwapTexture {
            path,
            bytes,
            variant,
        })
    }

    /// Shows `variant` of `path` (both copies are already on the thread). False as [`Viewport3d::swap_texture`].
    pub fn set_texture_variant(&mut self, path: String, variant: Variant) -> bool {
        self.send_texture_cmd(V3Cmd::SetTextureVariant { path, variant })
    }

    fn send_texture_cmd(&mut self, cmd: V3Cmd) -> bool {
        let Some(t) = self.content_thread() else {
            return false;
        };
        let what = match &cmd {
            V3Cmd::SwapTexture {
                path,
                bytes,
                variant,
            } => MarkKind::Swap {
                path: path.clone(),
                variant: *variant,
                bytes: bytes.len(),
            },
            V3Cmd::SetTextureVariant { variant, .. } => MarkKind::Flip {
                variant: *variant,
                paths: 1,
            },
            _ => return false,
        };
        if t.send(cmd).is_err() {
            return false;
        }
        self.mark(what);
        // The next request_frame draws it.
        self.last_sent = None;
        true
    }

    fn mark(&mut self, what: MarkKind) {
        let after = self.render_sent;
        // One click flips many paths: they share one line.
        if let (
            MarkKind::Flip { variant, paths },
            Some(Mark {
                after: a,
                what:
                    MarkKind::Flip {
                        variant: v,
                        paths: n,
                    },
                ..
            }),
        ) = (&what, self.marks.last_mut())
        {
            if *a == after && v == variant {
                *n += *paths;
                return;
            }
        }
        if self.marks.len() >= 64 {
            self.marks.remove(0);
        }
        self.marks.push(Mark {
            at: Instant::now(),
            after,
            what,
        });
    }

    /// A frame with id `id` reached the model: log every command it can show.
    fn answer_marks(&mut self, id: u64) {
        let (done, keep): (Vec<Mark>, Vec<Mark>) = std::mem::take(&mut self.marks)
            .into_iter()
            .partition(|m| m.after < id);
        self.marks = keep;
        for m in done {
            let ms = perf_log::ms_since(m.at);
            match m.what {
                MarkKind::Edit { shape, kind } => perf_log::emit(
                    "edit",
                    json!({"shape": shape, "kind": kind.map_or("unknown".into(), |k| format!("{k:?}")), "ms_to_frame": ms}),
                ),
                MarkKind::Swap {
                    path,
                    variant,
                    bytes,
                } => perf_log::emit(
                    "texture_swap",
                    json!({"path": path, "variant": format!("{variant:?}"), "bytes": bytes, "ms_to_frame": ms}),
                ),
                MarkKind::Flip { variant, paths } => perf_log::emit(
                    "ab_flip",
                    json!({"paths": paths, "variant": format!("{variant:?}"), "ms_to_frame": ms}),
                ),
            }
        }
    }

    /// The variant the next load's resolver bytes count as (the set preview's current mode). Announced to the
    /// thread with the next load, and only when it differs from what the thread has.
    pub fn set_load_variant(&mut self, v: Variant) {
        self.load_variant = v;
    }

    pub fn content(&self) -> Option<&Content> {
        self.content.as_ref()
    }

    fn ensure_thread(&mut self) -> bool {
        if self.thread.is_some() {
            return true;
        }
        let (Some(shaders), Some(cache)) = (self.shaders.clone(), self.cache_dir.clone()) else {
            return false;
        };
        let (ctx, crx) = channel();
        let features = self.features_on;
        let settings = self.settings.clone();
        let me = ctx.clone();
        spawn_guarded(self.tx.clone(), "viewport3d", move |tx| {
            render_loop(crx, me, tx, shaders, cache, features, settings)
        });
        self.load_variant_sent = Variant::Source; // a new thread starts at the default
        self.thread = Some(ctx);
        true
    }

    /// The panel wants a `w × h` frame of the current content and orbit: loads and renders only what changed.
    pub fn request_frame(&mut self, w: u32, h: u32) {
        if self.state == V3State::NotFound || matches!(self.state, V3State::Error(_)) {
            return;
        }
        if self.content.as_ref().is_some_and(Content::le_mesh) {
            return;
        }
        if self.content.is_none() || w == 0 || h == 0 || !self.confirmed || !self.ensure_thread() {
            return;
        }
        let Some(thread) = self.thread.clone() else {
            return;
        };
        if self.need_load {
            self.need_load = false;
            let id = self.id();
            let cmd = self
                .content
                .as_ref()
                .expect("content checked above")
                .load_cmd(id);
            if self.load_variant != self.load_variant_sent
                && thread
                    .send(V3Cmd::SetLoadVariant(self.load_variant))
                    .is_ok()
            {
                self.load_variant_sent = self.load_variant;
            }
            if thread.send(cmd).is_ok() {
                self.load_at = Some(Instant::now());
                // The load drops the queued renders and edits: their marks would never be answered.
                self.marks.clear();
                // A queued Render is dropped by the thread when this load reaches it, with no reply.
                self.render_done = self.render_sent;
                self.pending_load = Some(id);
                self.state = V3State::Rendering;
            }
            return;
        }
        let (Some(loaded), None) = (self.loaded, self.pending_load) else {
            return;
        };
        if self.held_frame || self.report.as_ref().is_some_and(|r| r.le_mesh) || !self.renderable()
        {
            return;
        }
        let camera = self.orbit.camera(self.bounds);
        let look = self.look;
        let want = (camera, w, h, loaded, look);
        if self.last_sent == Some(want) {
            return;
        }
        let id = self.id();
        if thread
            .send(V3Cmd::Render {
                id,
                camera,
                w,
                h,
                look,
            })
            .is_ok()
        {
            self.last_sent = Some(want);
            self.render_sent = id;
            self.state = V3State::Rendering;
        }
    }

    /// Something may be drawn. Parsed/Builtin content follows the current shapes: any not Hidden (a Failed shape keeps its old draw or draws nothing, and a compile failure must never freeze the panel); an edit can un-hide or repair the
    /// only shape); until the first `Shapes` reply the load report decides. Mesh: the load report.
    fn renderable(&self) -> bool {
        let n = self.report.as_ref().map_or(0, |r| r.shapes);
        match self.content {
            Some(Content::Parsed { .. } | Content::Builtin { .. }) if !self.shapes.is_empty() => {
                self.shapes.iter().any(|s| s.status != ShapeStatus::Hidden)
            }
            _ => n > 0,
        }
    }

    /// Drain replies. True when anything changed.
    pub fn poll(&mut self) -> bool {
        self.poll_at(Instant::now())
    }

    /// [`Viewport3d::poll`] with the clock given (the gap watch reads it).
    pub fn poll_at(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            match msg {
                WorkerMsg::Viewport3d(r) => self.handle(r),
                WorkerMsg::Panicked { what, message } => {
                    if what == "locate" {
                        self.locating = false;
                        self.notice = Some(format!("internal error in locate: {message}"));
                    } else {
                        self.thread = None;
                        self.pending_load = None;
                        self.render_done = self.render_sent;
                        self.edits_done = self.edits_sent;
                        self.state =
                            V3State::Error(format!("internal error in the 3D preview: {message}"));
                    }
                }
                _ => {}
            }
        }
        if changed && self.state == V3State::Rendering && !self.busy() {
            self.state = V3State::Ready;
        }
        // Work ended (replies are applied here): the next work starts its own spinner delay, drawn or not.
        if !self.busy() {
            self.busy_since = None;
        }
        // After the drain (a frame that takes the last reply asks for no next frame); the app re-arms after drawing.
        if let Some(gap) = self.gap.tick(now, self.busy()) {
            perf_log::emit("ui_gap", json!({"gap_ms": gap, "panel": self.panel}));
        }
        changed
    }

    /// Re-arms the gap watch at the end of the frame with the owner's busy (the condition that asks for the next
    /// frame): drawing may have started work (a render) after `poll`, and the set preview has its own.
    pub fn arm_gap(&mut self, busy: bool) {
        self.gap.arm(busy);
    }

    /// Applies one reply (what `poll` does with each message; public for tests).
    pub fn handle(&mut self, r: V3Reply) {
        match r {
            V3Reply::Ready { adapter, warp } => {
                perf_log::emit(
                    "gpu",
                    json!({"adapter": adapter, "warp": warp, "cs_root": self.shaders.as_ref().map(|f| f.root.display().to_string())}),
                );
                self.adapter = Some((adapter, warp));
            }
            V3Reply::Failed(e) => {
                self.thread = None;
                self.pending_load = None;
                self.render_done = self.render_sent;
                self.edits_done = self.edits_sent;
                self.state = V3State::Error(format!("3D preview unavailable: {e}"));
            }
            V3Reply::Loaded { id, report, bounds } => {
                if self.pending_load == Some(id) {
                    // A reload of the same mesh (shaders or features changed) keeps its id. When its permutations
                    // fail or are still compiling, the last frame stays (Review Focus #3).
                    let reload = self.loaded == Some(id) || self.kept_view;
                    let reload_failed = reload && !report.compile_errors.is_empty();
                    // Programs still compiling after a reload: load_parsed dropped the old draws, so the old
                    // image is all that is left to show (held until the new content is ready).
                    let reload_pending = reload && report.pending > 0;
                    self.pending_load = None;
                    self.loaded = Some(id);
                    self.held_frame = (reload_failed || reload_pending) && self.frame.is_some();
                    if (report.le_mesh || report.shapes == 0) && !reload_failed {
                        self.frame = None;
                    }
                    self.report = Some(report);
                    self.bounds = bounds;
                    self.last_sent = None;
                    if self.notice.as_deref() == Some(UPDATED_NOTICE) {
                        self.notice = None;
                    }
                } else if self.loaded == Some(id) && self.pending_load.is_none() {
                    // Reloaded after a device rebuild: same mesh, fresh report.
                    self.report = Some(report);
                    self.bounds = bounds;
                }
            }
            V3Reply::Frame { frame, timing } => {
                let fid = frame.id;
                self.render_done = self.render_done.max(frame.id);
                if self.loaded.is_some() && self.pending_load.is_none() {
                    self.render_error = None;
                    self.frame = Some(frame);
                    self.timing = Some(timing);
                    if let Some(t) = self.load_at.take() {
                        perf_log::emit(
                            "first_frame",
                            json!({"ms_since_load": perf_log::ms_since(t)}),
                        );
                    }
                    self.answer_marks(fid);
                }
            }
            V3Reply::Refresh { frame, timing } => {
                // Not an answered Render: render_done is untouched. Ignored while the old image is held.
                if self.loaded.is_some() && self.pending_load.is_none() && !self.held_frame {
                    self.render_error = None;
                    self.frame = Some(frame);
                    self.timing = Some(timing);
                }
            }
            V3Reply::RenderError { id, message } => {
                self.render_done = self.render_done.max(id);
                if self.loaded.is_some() && self.pending_load.is_none() {
                    self.render_error = Some(message);
                }
            }
            V3Reply::Shapes(s) => {
                // Only for the loaded content: a reply for content replaced since is dropped.
                if self.pending_load.is_none() && self.loaded.is_some() {
                    let first_time = |x: &ShapeInfo| {
                        matches!(
                            x.status,
                            ShapeStatus::Pending {
                                showing_previous: false
                            }
                        )
                    };
                    if s.iter().any(|x| x.status == ShapeStatus::Drawing)
                        && !s.iter().any(first_time)
                    {
                        self.held_frame = false;
                    }
                    self.shapes = s;
                }
            }
            V3Reply::MaterialSet {
                content,
                id,
                result,
                ms,
            } => {
                // A reply for content replaced since: its count was reset with the content.
                if self.loaded.or(self.pending_load) != Some(content) {
                    return;
                }
                self.edits_done = (self.edits_done + 1).min(self.edits_sent);
                match result {
                    Ok(kind) => {
                        for m in &mut self.marks {
                            if let MarkKind::Edit {
                                shape,
                                kind: k @ None,
                            } = &mut m.what
                            {
                                if *shape == id.0 {
                                    *k = Some(kind);
                                }
                            }
                        }
                        self.last_edit = Some((kind, ms));
                    }
                    Err(e) => self.render_error = Some(e),
                }
            }
            V3Reply::TextureSwap {
                content,
                path,
                result,
            } => {
                if self.loaded.or(self.pending_load) != Some(content) {
                    return;
                }
                if let Err(e) = result {
                    self.swap_errors.push(format!("{path}: {e}"));
                }
            }
            V3Reply::Located {
                generation,
                result,
                settings,
            } if generation == self.locate_gen => {
                self.on_located(result);
                if let Some(s) = settings {
                    // A Moved popup is still open: the new location's settings wait for "Use new".
                    if self.popup.is_none() {
                        self.on_settings(s);
                    } else {
                        self.popup_settings = Some(s);
                    }
                }
            }
            V3Reply::Chosen {
                generation,
                result,
                settings,
            } if generation == self.locate_gen => {
                self.on_chosen(result);
                if let Some(s) = settings {
                    self.on_settings(s);
                }
            }
            V3Reply::Located { .. } | V3Reply::Chosen { .. } => {}
            V3Reply::OffInPreview(off) => self.off_in_preview = off,
            V3Reply::Coverage(c) => self.coverage = c,
        }
    }
}

/// The `Shaders` folder for a folder the user picked: the folder itself when it holds `Lighting.hlsl`, else its
/// `Shaders` subfolder.
pub fn chosen_root(folder: &Path) -> Result<PathBuf, String> {
    if folder.join("Lighting.hlsl").is_file() {
        return Ok(folder.to_path_buf());
    }
    let s = folder.join("Shaders");
    if s.join("Lighting.hlsl").is_file() {
        return Ok(s);
    }
    Err(format!(
        "{} has no Lighting.hlsl (choose the Community Shaders mod folder or its Shaders folder).",
        folder.display()
    ))
}

/// The target's loose origins as `(name, root, priority)`: MO2 = game Data −1, mods by modlist order, overwrite
/// `i32::MAX`; a folder target is one origin.
fn target_origins(target: Option<&ScanTarget>) -> Vec<(String, PathBuf, i32)> {
    match target {
        Some(ScanTarget::Mo2 { root, profile }) => {
            modscan::mo2::Instance::load(root, Some(profile))
                .ok()
                .and_then(|i| modscan::mo2::loose_origins(&i).ok())
                .map(|(o, _)| {
                    o.into_iter()
                        .map(|o| (o.name, o.root, o.priority))
                        .collect()
                })
                .unwrap_or_default()
        }
        Some(ScanTarget::Folder(f)) => {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![(name, f.clone(), 0)]
        }
        None => vec![],
    }
}

/// The user's CS settings for the located `Shaders` folder (03 §3.4, C12). They come through the target's
/// origins (overwrite wins), also for a chosen folder; from the Data folder above `Shaders` only when no origin
/// has a settings file (no target, or none there). Never fails: problems become notes.
pub fn read_cs_settings(target: Option<&ScanTarget>, found: &Fingerprint) -> CsSettings {
    // Through the target's origins whatever folder the shaders came from (a chosen copy of CS still has its
    // settings in MO2's overwrite); the Data folder above `Shaders` only when no origin has a settings file.
    let origins = target_origins(target);
    let through_target = Some(origins.as_slice())
        .filter(|o| !o.is_empty())
        .map(|o| locate::settings_paths(Some(o), &found.root))
        .filter(|p| p.user.is_some() || p.default.is_some() || p.info_ini.is_some());
    let paths = through_target.unwrap_or_else(|| locate::settings_paths(None, &found.root));
    cs_settings::read_settings(&paths)
}

/// Finds the CS `Shaders` folder: a folder the user chose (no owning mod) while it still holds `Lighting.hlsl`;
/// else the highest-priority mod of the target providing `Shaders/Lighting.hlsl`; else the saved folder if it is
/// still there. Fingerprints it (reads every shader file).
pub fn locate_shaders(
    target: Option<&ScanTarget>,
    saved: Option<&Fingerprint>,
) -> Result<Option<Fingerprint>, String> {
    let fp = |root: &Path, owner: Option<String>| {
        locate::fingerprint(root, owner)
            .map(Some)
            .map_err(|e| e.to_string())
    };
    if let Some(s) = saved.filter(|s| s.owner_mod.is_none()) {
        if s.root.join("Lighting.hlsl").is_file() {
            return fp(&s.root, None);
        }
    }
    let origins = target_origins(target);
    if let Some((name, root)) = locate::find_in_mods(&origins) {
        return fp(&root.join("Shaders"), Some(name));
    }
    match saved {
        Some(s) if s.root.join("Lighting.hlsl").is_file() => fp(&s.root, s.owner_mod.clone()),
        _ => Ok(None),
    }
}

fn read_source(
    src: &FileSource,
    bsas: &RefCell<HashMap<PathBuf, Option<BsaIndex>>>,
) -> Result<Vec<u8>, String> {
    match src {
        FileSource::Loose(p) => std::fs::read(p).map_err(|e| format!("{}: {e}", p.display())),
        FileSource::Memory { bytes, .. } => Ok(bytes.to_vec()),
        FileSource::Optional(s) => read_source(s, bsas),
        FileSource::Bsa { archive, norm } => {
            let mut map = bsas.borrow_mut();
            let idx = map.entry(archive.clone()).or_insert_with(|| {
                vfs::bsa::Bsa::open(archive).ok().map(|b| {
                    let names = b
                        .entries
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (e.norm.clone(), i))
                        .collect();
                    BsaIndex { bsa: b, names }
                })
            });
            let Some(idx) = idx else {
                return Err(format!("{}: cannot open the archive", archive.display()));
            };
            let i = *idx
                .names
                .get(norm)
                .ok_or_else(|| format!("{norm}: not in {}", archive.display()))?;
            idx.bsa
                .read(&idx.bsa.entries[i])
                .map_err(|e| format!("{norm} in {}: {e}", archive.display()))
        }
    }
}

/// The bytes of one file (a fresh BSA index per call): for workers that read a single NIF.
pub fn read_file(src: &FileSource) -> Result<Vec<u8>, String> {
    read_source(src, &RefCell::new(HashMap::new()))
}

struct BsaIndex {
    bsa: vfs::bsa::Bsa,
    names: HashMap<String, usize>,
}

impl Drop for Viewport3d {
    fn drop(&mut self) {
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::Quit);
        }
    }
}

/// Applies user CS settings (C11: a cbuffer rewrite, recompiles only when the defines change, never a reload) and
/// writes the `settings` log line. The render thread calls it; public so a test can capture the line.
#[doc(hidden)]
pub fn apply_settings(r: &mut Renderer, s: CsSettings) {
    let t0 = Instant::now();
    let hash: String = s.hash.chars().take(12).collect();
    let notes = s.notes.len();
    r.set_settings(s);
    perf_log::emit(
        "settings",
        json!({"hash": hash, "defines": r.features(), "ms": perf_log::ms_since(t0), "notes": notes}),
    );
}

/// The texture resolver over `map` (canonical path → bytes); read errors are collected in `errors`.
fn resolver<'a>(
    map: &'a HashMap<String, FileSource>,
    bsas: &'a RefCell<HashMap<PathBuf, Option<BsaIndex>>>,
    errors: &'a RefCell<Vec<String>>,
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |norm: &str| {
        let src = map.get(norm)?;
        match read_source(src, bsas) {
            Ok(b) => Some(b),
            Err(_) if matches!(src, FileSource::Optional(_)) => None,
            Err(e) => {
                errors.borrow_mut().push(e);
                None
            }
        }
    }
}

/// Loads `c`, then applies the material edits made since its load again (a reload after a shaders, features or
/// device change keeps them).
fn load(r: &mut Renderer, c: &Content, edits: &BTreeMap<ShapeId, MaterialState>) -> LoadReport {
    let bsas = RefCell::new(HashMap::new());
    let errors = RefCell::new(vec![]);
    let textures = resolver(c.textures(), &bsas, &errors);
    let mut rep = match c {
        Content::Mesh(spec) => match read_source(&spec.nif, &bsas) {
            Ok(nif) => r.load(&nif, &textures),
            Err(e) => {
                return LoadReport {
                    errors: vec![format!("NIF: {e}")],
                    ..Default::default()
                }
            }
        },
        Content::Parsed { nif, .. } => r.load_parsed(nif, &textures),
        Content::Builtin {
            shape, material, ..
        } => r.load_builtin(*shape, material.clone(), &textures),
    };
    for (id, m) in edits {
        if let Err(e) = r.set_material(*id, m.clone(), &textures) {
            rep.errors.push(e);
        }
    }
    drop(textures);
    rep.errors.extend(errors.into_inner());
    rep
}

/// The render thread's texture state (P1-B A/B): the variant the load resolver's bytes count as, and the swaps and
/// flips made since the content's own load. A reload (shaders, features or device change) loads the content's own
/// bytes again, so they are applied again, exactly as `edits` are. A device rebuild drops the renderer's texture
/// cache, so the log holds both variants of every path swapped.
#[derive(Default)]
struct TexState {
    load: Variant,
    swaps: BTreeMap<(String, Variant), Arc<Vec<u8>>>,
    active: BTreeMap<String, Variant>,
}

impl TexState {
    fn clear_log(&mut self) {
        self.swaps.clear();
        self.active.clear();
    }

    /// Swaps first, then flips (the order the set preview sends them). A failure here would have failed the
    /// original swap too, which was reported then.
    fn replay(&self, r: &mut Renderer) {
        for ((path, v), bytes) in &self.swaps {
            let _ = r.replace_texture(path, bytes, *v);
        }
        for (path, v) in &self.active {
            r.set_texture_variant(path, *v);
        }
    }
}

enum TexCmd {
    Swap {
        path: String,
        bytes: Arc<Vec<u8>>,
        variant: Variant,
    },
    Flip {
        path: String,
        variant: Variant,
    },
}

/// [`load`] with the texture state: the load variant first, the logged swaps and flips after.
fn load_all(
    r: &mut Renderer,
    c: &Content,
    edits: &BTreeMap<ShapeId, MaterialState>,
    tex: &TexState,
) -> LoadReport {
    r.set_load_variant(tex.load);
    let rep = load(r, c, edits);
    tex.replay(r);
    rep
}

/// One material edit on the loaded content.
fn edit(r: &mut Renderer, c: &Content, id: ShapeId, m: MaterialState) -> Result<EditKind, String> {
    let bsas = RefCell::new(HashMap::new());
    let errors = RefCell::new(vec![]);
    let textures = resolver(c.textures(), &bsas, &errors);
    r.set_material(id, m, &textures)
}

/// The render thread: owns the `Renderer` and the compile pool; batches queued commands, applies material edits
/// after the load they follow, and draws only the latest camera. Compile results arrive on the same queue
/// (`V3Cmd::Compiled`, sent by the pool's sink), so a burst of them and a camera drag still collapse into one
/// render. A result that changes a shape makes a `Shapes` reply and, with no `Render` in the batch, a `Refresh`
/// frame from the last camera.
fn render_loop(
    rx: Receiver<V3Cmd>,
    self_tx: Sender<V3Cmd>,
    tx: &Sender<WorkerMsg>,
    shaders: Fingerprint,
    cache: PathBuf,
    features: bool,
    settings: CsSettings,
) {
    let send = |r: V3Reply| {
        let _ = tx.send(WorkerMsg::Viewport3d(r));
    };
    let make = |shaders: Fingerprint, features: bool| -> Result<Renderer, String> {
        let mut r = Renderer::new(shaders, cache.clone())?;
        r.set_async_compile(true);
        r.set_features_on(features);
        apply_settings(&mut r, settings.clone());
        Ok(r)
    };
    let loaded = |r: &Renderer, id: u64, c: &Content, report: LoadReport| {
        send(V3Reply::Loaded {
            id,
            report,
            bounds: r.bounds(),
        });
        if !matches!(c, Content::Mesh(_)) {
            send(V3Reply::Shapes(r.shapes()));
        }
    };
    let mut r = match make(shaders, features) {
        Ok(r) => r,
        Err(e) => return send(V3Reply::Failed(e)),
    };
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    send(V3Reply::OffInPreview(r.off_in_preview()));
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
        let _ = self_tx.send(V3Cmd::Compiled(d));
    });
    // Jobs queued by a load or an edit go to the workers.
    let pump = |r: &mut Renderer| {
        for job in r.take_jobs() {
            pool.submit(job);
        }
    };
    // After a device loss: a new device (maybe WARP) under the same renderer state (shaders, features and the
    // compiled programs are kept, so nothing recompiles), `Ready`, the last content loaded again with its
    // edits, and its `Loaded`. Err when no device can be made at all.
    let rebuild = |r: &mut Renderer,
                   last: &Option<(u64, Content)>,
                   edits: &BTreeMap<ShapeId, MaterialState>,
                   tex: &TexState|
     -> Result<(), String> {
        let t0 = std::time::Instant::now();
        r.rebuild_device_on(Gpu::new(None)?)?;
        send(V3Reply::Ready {
            adapter: r.gpu().adapter.clone(),
            warp: r.gpu().warp,
        });
        if let Some((id, c)) = last {
            let report = load_all(r, c, edits, tex);
            loaded(r, *id, c, report);
        }
        perf_log::emit("device_rebuild", json!({"ms": perf_log::ms_since(t0)}));
        Ok(())
    };
    let mut last: Option<(u64, Content)> = None;
    // Material edits applied to `last` since its load.
    let mut edits: BTreeMap<ShapeId, MaterialState> = BTreeMap::new();
    let mut tex = TexState::default();
    // The last camera, size and look drawn: what a compile-triggered refresh renders with.
    let mut view: Option<(Camera, u32, u32, Look)> = None;
    let mut refresh_id: u64 = 1 << 62;
    let mut stats = FrameStats::default();
    let mut last_coverage: Option<String> = None;
    while let Ok(first) = rx.recv() {
        let mut reload = false;
        let mut render = None;
        let mut compiled_changed = false;
        // A settings or features change: cbuffers rewritten, programs maybe swapped or queued; no reload.
        let mut restyled = false;
        let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];
        let mut tex_cmds: Vec<TexCmd> = vec![];
        for c in std::iter::once(first).chain(rx.try_iter()) {
            let content = match c {
                V3Cmd::Quit => return,
                V3Cmd::Compiled(done) => {
                    compiled_changed |= r.apply_compiled(done);
                    None
                }
                V3Cmd::SetShaders(f) => {
                    r.set_shaders(f);
                    reload = true;
                    None
                }
                V3Cmd::SetFeatures(on) => {
                    r.set_features_on(on);
                    restyled = true;
                    None
                }
                V3Cmd::SetSettings(s) => {
                    apply_settings(&mut r, s);
                    restyled = true;
                    None
                }
                V3Cmd::Load { id, spec } => Some((id, Content::Mesh(spec))),
                V3Cmd::LoadParsed { id, nif, textures } => Some((
                    id,
                    Content::Parsed {
                        key: id,
                        nif,
                        textures,
                    },
                )),
                V3Cmd::LoadBuiltin {
                    id,
                    shape,
                    material,
                    textures,
                } => Some((
                    id,
                    Content::Builtin {
                        key: id,
                        shape,
                        material,
                        textures,
                    },
                )),
                V3Cmd::SetMaterial {
                    content,
                    id,
                    material,
                } => {
                    pending.push((content, id, material));
                    None
                }
                V3Cmd::SetLoadVariant(v) => {
                    tex.load = v;
                    None
                }
                V3Cmd::SwapTexture {
                    path,
                    bytes,
                    variant,
                } => {
                    tex_cmds.push(TexCmd::Swap {
                        path,
                        bytes,
                        variant,
                    });
                    None
                }
                V3Cmd::SetTextureVariant { path, variant } => {
                    tex_cmds.push(TexCmd::Flip { path, variant });
                    None
                }
                V3Cmd::Render {
                    id,
                    camera,
                    w,
                    h,
                    look,
                } => {
                    render = Some((id, camera, w, h, look));
                    None
                }
            };
            if let Some(c) = content {
                // A new load drops the queued render and the edits made for the old content (the model reset its
                // counters when it replaced the content). Results already applied or still in flight for the old
                // content are keyed by program, not by shape: they only warm the memo.
                last = Some(c);
                edits.clear();
                pending.clear();
                tex_cmds.clear();
                tex.clear_log();
                render = None;
                reload = true;
            }
        }
        if reload {
            if let Some((id, c)) = &last {
                let t0 = std::time::Instant::now();
                let report = load_all(&mut r, c, &edits, &tex);
                perf_log::emit(
                    "load",
                    json!({"content": c.label(), "shapes": report.shapes, "pending": report.pending, "build_ms": perf_log::ms_since(t0)}),
                );
                if r.gpu().is_removed() {
                    // Lost during the load: rebuild and load again now; with no shapes no Render would follow to
                    // trigger the rebuild.
                    if let Err(e) = rebuild(&mut r, &last, &edits, &tex) {
                        return send(V3Reply::Failed(e));
                    }
                } else {
                    loaded(&r, *id, c, report);
                }
            }
        }
        let pending_was_empty = pending.is_empty();
        if !pending.is_empty() {
            if let Some((lid, c)) = &last {
                for (content, id, m) in pending {
                    if content != *lid {
                        continue;
                    }
                    let t0 = std::time::Instant::now();
                    let result = edit(&mut r, c, id, m.clone());
                    if result.is_ok() {
                        edits.insert(id, m);
                    }
                    send(V3Reply::MaterialSet {
                        content,
                        id,
                        result,
                        ms: t0.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                send(V3Reply::Shapes(r.shapes()));
            }
        }
        if !tex_cmds.is_empty() {
            if let Some((lid, _)) = &last {
                for t in tex_cmds {
                    match t {
                        TexCmd::Swap {
                            path,
                            bytes,
                            variant,
                        } => {
                            let result = r.replace_texture(&path, &bytes, variant).map(|_| ());
                            if result.is_ok() {
                                tex.swaps.insert((path.clone(), variant), bytes);
                            }
                            send(V3Reply::TextureSwap {
                                content: *lid,
                                path,
                                result,
                            });
                        }
                        TexCmd::Flip { path, variant } => {
                            r.set_texture_variant(&path, variant);
                            tex.active.insert(path, variant);
                        }
                    }
                }
            }
        }
        pump(&mut r);
        // A settings apply or a new program can change it; sent only on change.
        let coverage = r.settings_coverage().line();
        if coverage != last_coverage {
            send(V3Reply::Coverage(coverage.clone()));
            last_coverage = coverage;
        }
        if restyled {
            send(V3Reply::OffInPreview(r.off_in_preview()));
        }
        // Redraw from the last camera, and report the shapes (now pending or swapped) like a landed program.
        compiled_changed |= restyled;
        // A landed program changed a shape and nothing above reported it: tell the panel.
        if compiled_changed
            && !reload
            && pending_was_empty
            && !matches!(last, Some((_, Content::Mesh(_))))
        {
            send(V3Reply::Shapes(r.shapes()));
        }
        let (id, camera, w, h, look, refresh) = match (render, view) {
            (Some((id, camera, w, h, look)), _) => (id, camera, w, h, look, false),
            (None, Some((camera, w, h, look))) if compiled_changed => {
                refresh_id += 1;
                (refresh_id, camera, w, h, look, true)
            }
            _ => continue,
        };
        view = Some((camera, w, h, look));
        let scene = look.scene(camera, r.bounds());
        let mut result = r.render(&scene, w, h);
        if let Err(e) = &result {
            if e.starts_with(render3d::device::DEVICE_REMOVED) {
                // Device lost (TDR, driver update): a new device (maybe WARP), the content again, one retry.
                if let Err(e) = rebuild(&mut r, &last, &edits, &tex) {
                    return send(V3Reply::Failed(e));
                }
                pump(&mut r);
                result = r.render(&scene, w, h);
            }
        }
        match result {
            Ok(rgba) => {
                let frame = Arc::new(Frame {
                    width: w,
                    height: h,
                    rgba,
                    id,
                });
                let timing = r.last_timing();
                if let Some(s) = stats.push(
                    std::time::Instant::now(),
                    timing.draw_ms,
                    timing.draw_ms + timing.gpu_ms + timing.readback_ms,
                ) {
                    perf_log::emit("frame_stats", s);
                }
                if refresh {
                    send(V3Reply::Refresh { frame, timing });
                } else {
                    send(V3Reply::Frame { frame, timing });
                }
            }
            // A failed refresh is dropped: the next real Render reports the error with an id the model knows.
            Err(message) if !refresh => send(V3Reply::RenderError { id, message }),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P1-C Task 8 fix 1: an RFAOS candidate whose file is gone is quiet (no load-report error) and the other
    /// candidate is served; an ordinary source that fails is still an error.
    #[test]
    fn a_failed_optional_source_is_quiet() {
        let gone = || FileSource::Loose(PathBuf::from(r"C:\nope\sdc\gone_rfaos.dds"));
        let map = HashMap::from([
            (
                "a_rfaos".to_string(),
                FileSource::Optional(Box::new(gone())),
            ),
            (
                "b_rfaos".to_string(),
                FileSource::Optional(Box::new(FileSource::Memory {
                    id: 1,
                    bytes: Arc::new(b"dds".to_vec()),
                })),
            ),
            ("slot".to_string(), gone()),
        ]);
        let (bsas, errors) = (RefCell::new(HashMap::new()), RefCell::new(vec![]));
        let r = resolver(&map, &bsas, &errors);
        assert_eq!(r("a_rfaos"), None);
        assert_eq!(r("b_rfaos").as_deref(), Some(&b"dds"[..]));
        assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
        assert_eq!(r("slot"), None);
        assert_eq!(
            errors.borrow().len(),
            1,
            "a slot texture that fails is reported"
        );
    }
}
