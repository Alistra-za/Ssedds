//! The 3D viewport of the Meshes view (plan B Task B8): the selected NIF drawn with the user's Community Shaders
//! `Lighting.hlsl` by `render3d` (D3D11), and the CS-folder locate flow (00 §6.4).
//!
//! The `Renderer` lives on one long-lived worker thread (D3D11's immediate context is single-threaded); this model
//! sends it commands and drains its replies (`WorkerMsg::Viewport3d`) on the UI thread. Locating CS hashes every
//! shader file, so it runs on a short-lived worker too. The model holds pixels, never egui textures: `ui::viewport3d`
//! uploads a frame when its id changes.
use crate::model::meshes::ScanTarget;
use crate::worker::{spawn_guarded, WorkerMsg};
use render3d::locate::{self, Change, Fingerprint};
use render3d::scene::{Camera, Scene, AUTO_DISTANCE, AUTO_PITCH, AUTO_YAW};
use render3d::{FrameTiming, LoadReport, Renderer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Panel text when no CS install is found (00 §6.4).
pub const NOT_FOUND: &str = "Community Shaders not found";
pub const CHOOSE_FOLDER: &str = "Choose folder…";
pub const USE_NEW: &str = "Use new";
pub const KEEP_OLD: &str = "Keep old";
/// The one-line notice for a hash-only change.
pub const UPDATED_NOTICE: &str = "Community Shaders updated — recompiling shaders…";
/// The CS-features toggle (on by default). Off compiles without CS feature defines (P0: no `ENABLE_LL`).
pub const FEATURES_LABEL: &str = "Community Shaders features";
pub const CUBEMAPS_FOOTER: &str = "Dynamic Cubemaps: off in preview";
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

/// Where a file's bytes are: a loose file, or an entry of a BSA.
#[derive(Debug, Clone, PartialEq)]
pub enum FileSource {
    Loose(PathBuf),
    Bsa { archive: PathBuf, norm: String },
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

/// Commands to the render thread.
pub enum V3Cmd {
    SetShaders(Fingerprint),
    SetFeatures(bool),
    Load {
        id: u64,
        spec: Arc<MeshSpec>,
    },
    Render {
        id: u64,
        camera: Camera,
        w: u32,
        h: u32,
    },
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
    RenderError {
        id: u64,
        message: String,
    },
    /// A locate answer; `generation` tells a stale one (started before a later locate or "Choose folder…").
    Located {
        generation: u64,
        result: Result<Option<Fingerprint>, String>,
    },
    Chosen {
        generation: u64,
        result: Result<Fingerprint, String>,
    },
}

/// `%LOCALAPPDATA%\sdc\shader-cache`.
pub fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("sdc").join("shader-cache"))
}

pub struct Viewport3d {
    pub state: V3State,
    pub orbit: Orbit,
    pub report: Option<LoadReport>,
    /// The moved/renamed question (`Change::Moved`).
    pub popup: Option<Change>,
    pub features_on: bool,
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
    pub render_error: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub bounds: Option<([f32; 3], f32)>,
    mesh: Option<Arc<MeshSpec>>,
    need_load: bool,
    loaded: Option<u64>,
    pending_load: Option<u64>,
    last_sent: Option<(Camera, u32, u32, u64)>,
    render_sent: u64,
    render_done: u64,
    next_id: u64,
    locating: bool,
    locate_gen: u64,
    declined: Option<PathBuf>,
    shaders_changed: bool,
    thread: Option<Sender<V3Cmd>>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl Viewport3d {
    /// `saved` is `[render3d] shaders` from `gui.toml`.
    pub fn new(saved: Option<Fingerprint>) -> Viewport3d {
        let (tx, rx) = channel();
        Viewport3d {
            state: V3State::Ready,
            orbit: Orbit::default(),
            report: None,
            popup: None,
            features_on: true,
            notice: None,
            shaders: saved,
            frame: None,
            timing: None,
            upload_ms: None,
            adapter: None,
            render_error: None,
            cache_dir: default_cache_dir(),
            bounds: None,
            mesh: None,
            need_load: false,
            loaded: None,
            pending_load: None,
            last_sent: None,
            render_sent: 0,
            render_done: 0,
            next_id: 0,
            locating: false,
            locate_gen: 0,
            declined: None,
            shaders_changed: false,
            thread: None,
            tx,
            rx,
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

    /// Test seam: stands in for the render thread; the returned receiver gets the commands.
    #[doc(hidden)]
    pub fn attach_test_thread(&mut self) -> Receiver<V3Cmd> {
        let (t, r) = channel();
        self.thread = Some(t);
        r
    }

    pub fn locating(&self) -> bool {
        self.locating
    }

    /// A load, a render or a locate is in flight.
    pub fn busy(&self) -> bool {
        self.locating || self.pending_load.is_some() || self.render_done < self.render_sent
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
        self.locating = true;
        self.locate_gen += 1;
        let generation = self.locate_gen;
        let saved = self.shaders.clone();
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = locate_shaders(target.as_ref(), saved.as_ref());
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Located {
                generation,
                result,
            }));
        });
    }

    /// "Choose folder…": a CS mod folder or its `Shaders` folder.
    pub fn choose_folder(&mut self, folder: PathBuf) {
        self.locating = true;
        self.locate_gen += 1;
        let generation = self.locate_gen;
        spawn_guarded(self.tx.clone(), "locate", move |tx| {
            let result = chosen_root(&folder)
                .and_then(|root| locate::fingerprint(&root, None).map_err(|e| e.to_string()));
            let _ = tx.send(WorkerMsg::Viewport3d(V3Reply::Chosen {
                generation,
                result,
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
            self.adopt(new);
        }
    }

    /// "Keep old": keep drawing with the old folder and don't ask about this new one again this session.
    pub fn keep_old(&mut self) {
        if let Some(Change::Moved { new, .. }) = self.popup.take() {
            self.declined = Some(new.root);
        }
    }

    fn adopt(&mut self, f: Fingerprint) {
        self.popup = None;
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

    /// The "Community Shaders features" toggle.
    pub fn set_features(&mut self, on: bool) {
        if self.features_on == on {
            return;
        }
        self.features_on = on;
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::SetFeatures(on));
            self.reload_pending();
        }
    }

    /// The mesh the Meshes view shows (`None`: nothing selected). Loaded on the next `request_frame`.
    pub fn set_mesh(&mut self, spec: Option<MeshSpec>) {
        if self.mesh.as_deref() == spec.as_ref() {
            return;
        }
        self.mesh = spec.map(Arc::new);
        let le = self.mesh.as_ref().is_some_and(|m| m.le_mesh);
        self.need_load = self.mesh.is_some() && !le;
        self.loaded = None;
        self.pending_load = None;
        // Renders in flight are for the old mesh; the thread drops one queued behind a new Load without a reply.
        self.render_done = self.render_sent;
        self.report = le.then(|| LoadReport {
            le_mesh: true,
            ..Default::default()
        });
        self.bounds = None;
        self.frame = None;
        self.render_error = None;
        self.last_sent = None;
        self.orbit.reset();
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
        spawn_guarded(self.tx.clone(), "viewport3d", move |tx| {
            render_loop(crx, tx, shaders, cache, features)
        });
        self.thread = Some(ctx);
        true
    }

    /// The panel wants a `w × h` frame of the current mesh and orbit: loads and renders only what changed.
    pub fn request_frame(&mut self, w: u32, h: u32) {
        if self.state == V3State::NotFound || matches!(self.state, V3State::Error(_)) {
            return;
        }
        if self.mesh.as_ref().is_some_and(|m| m.le_mesh) {
            return;
        }
        if self.mesh.is_none() || w == 0 || h == 0 || !self.ensure_thread() {
            return;
        }
        let Some(thread) = self.thread.clone() else {
            return;
        };
        if self.need_load {
            self.need_load = false;
            let id = self.id();
            let spec = self.mesh.clone().expect("mesh checked above");
            if thread.send(V3Cmd::Load { id, spec }).is_ok() {
                // A queued Render is dropped by the thread when this Load reaches it, with no reply.
                self.render_done = self.render_sent;
                self.pending_load = Some(id);
                self.state = V3State::Rendering;
            }
            return;
        }
        let (Some(loaded), None) = (self.loaded, self.pending_load) else {
            return;
        };
        if self
            .report
            .as_ref()
            .is_some_and(|r| r.le_mesh || r.shapes == 0)
        {
            return;
        }
        let camera = self.orbit.camera(self.bounds);
        let want = (camera, w, h, loaded);
        if self.last_sent == Some(want) {
            return;
        }
        let id = self.id();
        if thread.send(V3Cmd::Render { id, camera, w, h }).is_ok() {
            self.last_sent = Some(want);
            self.render_sent = id;
            self.state = V3State::Rendering;
        }
    }

    /// Drain replies. True when anything changed.
    pub fn poll(&mut self) -> bool {
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
        changed
    }

    /// Applies one reply (what `poll` does with each message; public for tests).
    pub fn handle(&mut self, r: V3Reply) {
        match r {
            V3Reply::Ready { adapter, warp } => self.adapter = Some((adapter, warp)),
            V3Reply::Failed(e) => {
                self.thread = None;
                self.pending_load = None;
                self.render_done = self.render_sent;
                self.state = V3State::Error(format!("3D preview unavailable: {e}"));
            }
            V3Reply::Loaded { id, report, bounds } => {
                if self.pending_load == Some(id) {
                    self.pending_load = None;
                    self.loaded = Some(id);
                    if report.le_mesh || report.shapes == 0 {
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
                self.render_done = self.render_done.max(frame.id);
                if self.loaded.is_some() && self.pending_load.is_none() {
                    self.render_error = None;
                    self.frame = Some(frame);
                    self.timing = Some(timing);
                }
            }
            V3Reply::RenderError { id, message } => {
                self.render_done = self.render_done.max(id);
                self.render_error = Some(message);
            }
            V3Reply::Located { generation, result } if generation == self.locate_gen => {
                self.on_located(result)
            }
            V3Reply::Chosen { generation, result } if generation == self.locate_gen => {
                self.on_chosen(result)
            }
            V3Reply::Located { .. } | V3Reply::Chosen { .. } => {}
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
    let origins: Vec<(String, PathBuf, i32)> = match target {
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
    };
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

struct BsaIndex {
    bsa: vfs::bsa::Bsa,
    names: HashMap<String, usize>,
}

fn apply_features(r: &mut Renderer, on: bool) {
    let f = if on {
        render3d::DEFAULT_FEATURES
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![]
    };
    // Linear Lighting draws in linear space: encode for display. Without it the output is gamma-space already.
    let ll = f.iter().any(|s: &String| s == "ENABLE_LL");
    r.set_features(f);
    r.set_encode_srgb(ll);
}

fn load(r: &mut Renderer, spec: &MeshSpec) -> LoadReport {
    let bsas = RefCell::new(HashMap::new());
    let nif = match read_source(&spec.nif, &bsas) {
        Ok(b) => b,
        Err(e) => {
            return LoadReport {
                errors: vec![format!("NIF: {e}")],
                ..Default::default()
            }
        }
    };
    let errors = RefCell::new(vec![]);
    let textures = |norm: &str| -> Option<Vec<u8>> {
        let src = spec.textures.get(norm)?;
        match read_source(src, &bsas) {
            Ok(b) => Some(b),
            Err(e) => {
                errors.borrow_mut().push(e);
                None
            }
        }
    };
    let mut rep = r.load(&nif, &textures);
    rep.errors.extend(errors.into_inner());
    rep
}

/// The render thread: owns the `Renderer`; batches queued commands and draws only the latest camera.
fn render_loop(
    rx: Receiver<V3Cmd>,
    tx: &Sender<WorkerMsg>,
    shaders: Fingerprint,
    cache: PathBuf,
    features: bool,
) {
    let send = |r: V3Reply| {
        let _ = tx.send(WorkerMsg::Viewport3d(r));
    };
    let make = |shaders: Fingerprint, features: bool| -> Result<Renderer, String> {
        let mut r = Renderer::new(shaders, cache.clone())?;
        apply_features(&mut r, features);
        Ok(r)
    };
    let mut features = features;
    let mut r = match make(shaders, features) {
        Ok(r) => r,
        Err(e) => return send(V3Reply::Failed(e)),
    };
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    let mut last: Option<(u64, Arc<MeshSpec>)> = None;
    while let Ok(first) = rx.recv() {
        let mut reload = false;
        let mut render = None;
        for c in std::iter::once(first).chain(rx.try_iter()) {
            match c {
                V3Cmd::SetShaders(f) => {
                    r.set_shaders(f);
                    reload = true;
                }
                V3Cmd::SetFeatures(on) => {
                    features = on;
                    apply_features(&mut r, on);
                    reload = true;
                }
                V3Cmd::Load { id, spec } => {
                    last = Some((id, spec));
                    render = None;
                    reload = true;
                }
                V3Cmd::Render { id, camera, w, h } => render = Some((id, camera, w, h)),
            }
        }
        if reload {
            if let Some((id, spec)) = &last {
                let report = load(&mut r, spec);
                send(V3Reply::Loaded {
                    id: *id,
                    report,
                    bounds: r.bounds(),
                });
            }
        }
        let Some((id, camera, w, h)) = render else {
            continue;
        };
        let scene = Scene {
            camera,
            ..Scene::neutral()
        };
        let mut result = r.render(&scene, w, h);
        if let Err(e) = &result {
            if e.starts_with(render3d::device::DEVICE_REMOVED) {
                // Device lost (TDR, driver update): a new device (maybe WARP), the mesh again, one retry.
                match make(r.shaders().clone(), features) {
                    Ok(nr) => {
                        r = nr;
                        send(V3Reply::Ready {
                            adapter: r.gpu().adapter.clone(),
                            warp: r.gpu().warp,
                        });
                        if let Some((lid, spec)) = &last {
                            let report = load(&mut r, spec);
                            send(V3Reply::Loaded {
                                id: *lid,
                                report,
                                bounds: r.bounds(),
                            });
                        }
                        result = r.render(&scene, w, h);
                    }
                    Err(e) => return send(V3Reply::Failed(e)),
                }
            }
        }
        match result {
            Ok(rgba) => send(V3Reply::Frame {
                frame: Arc::new(Frame {
                    width: w,
                    height: h,
                    rgba,
                    id,
                }),
                timing: r.last_timing(),
            }),
            Err(message) => send(V3Reply::RenderError { id, message }),
        }
    }
}
