//! The live set preview (P1-A, spec §2.3, 02): the Textures view's own 3D panel, showing the selected set on a game
//! mesh patched in memory as PGPatcher 1.2.0 would patch it (`pgpsim`), with this session's encodes of the set's
//! textures. No egui types; `ui::set_preview` draws it.
//!
//! Part 1: per-member preview encodes → DDS bytes served as `FileSource::Memory`, fresh/stale per member, paused
//! during an export. Results come back on this module's own channel, so `worker.rs` needs no new message.
use crate::model::items::{ItemKind, ItemStatus, TextureItem};
use crate::model::meshes::ScanTarget;
use crate::model::pbr_export::json_entries;
use crate::model::preview::DEBOUNCE;
use crate::model::sets::set_views;
use crate::model::sets::{SetView, SlotFill, SlotKind};
use crate::model::viewport3d::FileSource;
use crate::model::viewport3d::{read_file, Content, Viewport3d};
use crate::model::AppModel;
use crate::model::Selection;
use crate::pgp::prefix::output_rel;
use crate::pgp::writer::EntryInput;
use crate::pgp::writer::{document, entry_node};
use crate::settings::MeshesSettings;
use engine::api::CancelToken;
use engine::config::Format;
use engine::dds::{
    dds_bytes, dxgi_format, resolve_header, DdsDesc, DDPF_FOURCC, DDSCAPS_COMPLEX, DDSCAPS_MIPMAP,
    DDSCAPS_TEXTURE, DDSD_CAPS, DDSD_HEIGHT, DDSD_MIPMAPCOUNT, DDSD_PITCH, DDSD_PIXELFORMAT,
    DDSD_WIDTH,
};
use engine::image::Rgba32F;
use engine::manifest::sha256_json;
use engine::policy::ResolvedJob;
use engine::session::{pack_key, Session};
use modscan::store::{MatchRow, ShapeView, Store, TexKey};
use nif::material::MaterialState;
use nif::path::TexPath;
use pgpsim::{Badge, SimInput, SimResult};
use render3d::builtin::Builtin;
use render3d::perf_log;
use render3d::{ParsedNif, ShapeId, Variant};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

/// Memory ids are process-wide: `FileSource::Memory` compares by id, across encoders.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// D5: the preview's encode size cap (`[render3d] set_preview_max` in `gui.toml`, 0 = this).
pub const DEFAULT_CAP: u32 = 2048;
pub const WAITS_FOR_EXPORT: &str = "Preview encoding waits for the export to finish";
const BLOCKED_ERROR: &str = "the texture has an error";
const BLOCKED_NOTHING: &str = "nothing to encode";

/// What a member's bytes are made of: the engine's full export path, or its prepared source chain uncompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PreviewMode {
    #[default]
    Encoded,
    Source,
}

pub const MODE_LABEL_ENCODED: &str = "Encoded (exact export)";
pub const MODE_LABEL_SOURCE: &str = "Source (faster, no compression)";

impl PreviewMode {
    pub fn other(self) -> PreviewMode {
        match self {
            PreviewMode::Encoded => PreviewMode::Source,
            PreviewMode::Source => PreviewMode::Encoded,
        }
    }
}

/// The renderer's variant for a mode (P1-B contract: `Encoded` <-> `Variant::Encoded`, `Source` <-> `Variant::Source`).
pub fn variant_of(m: PreviewMode) -> Variant {
    match m {
        PreviewMode::Encoded => Variant::Encoded,
        PreviewMode::Source => Variant::Source,
    }
}

pub fn cap_text(cap: u32) -> String {
    format!("Preview at {cap} px, export writes full size")
}

/// The canonical render path (`textures/…`, lower case, `/`) of a texture path in any form.
pub fn canon(p: &str) -> String {
    nif::path::canon_texture(p.as_bytes()).norm
}

/// Where a set the simulation cannot preview (D3) puts its textures: `textures/sdcpreview/<suffix>.dds`.
pub fn plain_path(kind: SlotKind) -> String {
    let s = kind.suffix();
    format!(
        "textures/sdcpreview/{}.dds",
        if s.is_empty() { "d" } else { s }
    )
}

/// What a slot member can give the preview.
#[derive(Debug, Clone)]
pub enum Source {
    Job(Box<ResolvedJob>),
    /// A DDS input: its own bytes (no encode, no cap).
    Dds(PathBuf),
    Blocked(&'static str),
}

/// 02 §2.3, mirroring `preview::classify`: a job to encode (`NeedsDecision` too: the suggested settings), a DDS
/// input's bytes, or nothing (an error, skipped, failed, or a raw map packed into another output).
pub fn source_of(item: &TextureItem) -> Source {
    if item.kind == ItemKind::Dds {
        return Source::Dds(item.input.clone());
    }
    if matches!(item.status, ItemStatus::Error(_)) {
        return Source::Blocked(BLOCKED_ERROR);
    }
    match &item.job {
        Some(job)
            if !matches!(
                item.status,
                ItemStatus::Skipped(_) | ItemStatus::Failed(_) | ItemStatus::PackedInto(_)
            ) =>
        {
            Source::Job(Box::new(job.clone()))
        }
        _ => Source::Blocked(BLOCKED_NOTHING),
    }
}

/// What makes the member's bytes: the profile plus the pack key or the source stamp (`preview.rs`'s private
/// `signature`, re-implemented from pub engine items, 02 §2.4); for a DDS input its path and stamp.
pub fn signature(item: &TextureItem) -> String {
    match &item.job {
        Some(job) if item.kind != ItemKind::Dds => {
            let p = sha256_json(&job.profile).unwrap_or_default();
            match &job.pack {
                Some(pk) => format!("{p}:{}", pack_key(pk).unwrap_or_default()),
                None => format!(
                    "{p}:{:?}:{:?}:{:?}",
                    job.input,
                    job.source,
                    job.active_rnm()
                ),
            }
        }
        _ => format!("{:?}:{:?}:{:?}", item.kind, item.input, item.stamp),
    }
}

/// One filled slot of the selected set: its kind, the member item, what its bytes depend on, and every canonical
/// path the preview serves those bytes under.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberSlot {
    pub kind: SlotKind,
    pub item: String,
    pub job_key: String,
    /// `ItemStatus` as text: a decision changes what is encoded.
    pub status: String,
    pub sig: String,
    pub paths: Vec<String>,
}

/// The set's filled slots (02 §1.1). With the PGPatcher entry (CS and a PBR set): the slot's path in
/// `entry.slots[slotN]` when sdc names it there (a borrowed slot, or a file another set owns, `pbr_export::slots_of`),
/// and for the set's own texture `<prefix>_<suffix>.dds`. A user's own `slotN` setting points at some other file
/// and is not served. Without the entry (D3): [`plain_path`]. Coat and fuzz share slot 7: both claim an explicit
/// slot-7 path, and the later one wins in [`Encoder::memory_sources`].
pub fn member_slots(m: &AppModel, v: &SetView, entry: Option<&EntryInput>) -> Vec<MemberSlot> {
    let mut out = vec![];
    for (kind, fill) in &v.slots {
        let (key, own) = match fill {
            SlotFill::Own { item } => (item, true),
            SlotFill::Borrowed { item, .. } => (item, false),
            SlotFill::Empty => continue,
        };
        let Some(item) = m.item(key) else {
            continue;
        };
        let mut paths = vec![];
        match entry {
            None => paths.push(plain_path(*kind)),
            Some(e) => {
                let n = kind.slot_number();
                let user = v
                    .settings
                    .get(&format!("slot{n}"))
                    .is_some_and(|x| x.is_string());
                if let Some(p) = e.slots.get(&n).filter(|p| !user && !p.is_empty()) {
                    paths.push(canon(p));
                }
                if let (true, Some(pre)) = (own, &v.prefix) {
                    let p = canon(&output_rel(pre, kind.suffix()).to_string_lossy());
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
            }
        }
        out.push(MemberSlot {
            kind: *kind,
            item: key.clone(),
            job_key: item.job_key.clone(),
            status: format!("{:?}", item.status),
            sig: signature(item),
            paths,
        });
    }
    out
}

/// A finished preview encode: a whole DDS (legacy or DX10 header, as export writes it) and its memory id.
#[derive(Debug)]
pub struct Encoded {
    pub id: u64,
    pub dds: Arc<Vec<u8>>,
    pub ms: f64,
}

#[derive(Debug, Clone)]
pub enum EncState {
    NotEncoded,
    /// Changed: waits for the debounce (or the export); the previous bytes stay on show.
    Stale(Option<Arc<Encoded>>),
    Encoding(Option<Arc<Encoded>>),
    Fresh(Arc<Encoded>),
    Failed(String),
    Blocked(&'static str),
}

impl EncState {
    /// The bytes on show: fresh ones, or the previous ones while stale or encoding.
    pub fn shown(&self) -> Option<&Arc<Encoded>> {
        match self {
            EncState::Stale(e) | EncState::Encoding(e) => e.as_ref(),
            EncState::Fresh(e) => Some(e),
            _ => None,
        }
    }
    /// Nothing more happens until the member changes.
    pub fn settled(&self) -> bool {
        matches!(
            self,
            EncState::Fresh(_) | EncState::Failed(_) | EncState::Blocked(_)
        )
    }
    pub fn label(&self) -> String {
        match self {
            EncState::NotEncoded => "not encoded yet".into(),
            EncState::Stale(_) => "stale".into(),
            EncState::Encoding(_) => "encoding…".into(),
            EncState::Fresh(e) => format!("fresh ({:.0} ms)", e.ms),
            EncState::Failed(m) => format!("failed: {m}"),
            EncState::Blocked(w) => format!("not previewed: {w}"),
        }
    }
}

#[derive(Debug)]
pub struct Member {
    pub slot: MemberSlot,
    pub state: EncState,
    source: Source,
    due: Option<Instant>,
    /// The live encode's generation; 0 = none (every change that supersedes an encode zeroes it).
    token: u64,
    /// The cap the live encode runs at, and the cap the shown bytes were made at.
    cap: u32,
    /// The mode of the live encode, and of the shown bytes (as `cap`).
    mode: PreviewMode,
    cancel: Option<CancelToken>,
    /// A/B: the other mode's Fresh bytes at the current cap and signature (a DDS input: the same bytes).
    alt: Option<Arc<Encoded>>,
    /// The other mode's background encode: its generation (0 = none) and cancel, and whether it failed (not retried
    /// until the member changes).
    alt_token: u64,
    alt_cancel: Option<CancelToken>,
    alt_err: bool,
}

struct EncDone {
    kind: SlotKind,
    token: u64,
    cap: u32,
    mode: PreviewMode,
    result: Result<(Vec<u8>, f64), String>,
}

/// What one encode run is for.
#[derive(Clone, Copy)]
struct Run {
    kind: SlotKind,
    token: u64,
    cap: u32,
    mode: PreviewMode,
}

fn spawn_encode(
    tx: Sender<EncDone>,
    session: Arc<Session>,
    source: Source,
    run: Run,
    cancel: CancelToken,
) {
    spawn(
        tx,
        move || {
            let t0 = Instant::now();
            let result = encode(&session, &source, run.cap, run.mode, &cancel).map(|b| (b, ms(t0)));
            EncDone {
                kind: run.kind,
                token: run.token,
                cap: run.cap,
                mode: run.mode,
                result,
            }
        },
        move |e| EncDone {
            kind: run.kind,
            token: run.token,
            cap: run.cap,
            mode: run.mode,
            result: Err(format!("internal error in a preview encode: {e}")),
        },
    );
}

impl Member {
    fn cancel_all(&self) {
        for c in [&self.cancel, &self.alt_cancel].into_iter().flatten() {
            c.cancel();
        }
    }

    /// The other mode's copy and encode are void (source, settings or cap changed).
    fn clear_alt(&mut self) {
        if let Some(c) = self.alt_cancel.take() {
            c.cancel();
        }
        self.alt = None;
        self.alt_token = 0;
        self.alt_err = false;
    }

    /// The mode flips. The other copy is Fresh: swap the two states, no work. Otherwise as before the A/B (the
    /// member goes stale, old bytes on show, encodes again), except that the old Fresh bytes are kept as the other
    /// copy and an other-mode encode already running becomes the live one.
    fn flip(&mut self, mode: PreviewMode, cap: u32, now: Instant) {
        if let Some(alt) = self.alt.take() {
            if let Some(c) = self.cancel.take() {
                c.cancel();
            }
            self.token = 0;
            self.due = None;
            let old = std::mem::replace(&mut self.state, EncState::Fresh(alt));
            self.alt = match old {
                EncState::Fresh(e) => Some(e),
                _ => None,
            };
            self.mode = mode;
            self.cap = cap;
            return;
        }
        let kept = match &self.state {
            EncState::Fresh(e) => Some(e.clone()),
            _ => None,
        };
        if let Some(c) = self.cancel.take() {
            c.cancel();
        }
        self.token = 0;
        if matches!(self.state, EncState::Blocked(_)) {
            return;
        }
        let shown = self.state.shown().cloned();
        self.cap = cap;
        self.mode = mode;
        if self.alt_token != 0 {
            self.token = std::mem::take(&mut self.alt_token);
            self.cancel = self.alt_cancel.take();
            self.due = None;
            self.state = EncState::Encoding(shown);
        } else {
            self.state = EncState::Stale(shown);
            self.due = Some(now);
        }
        self.alt = kept;
        self.alt_err = false;
    }
}

/// One `encode` line in the timing log for a finished encode.
fn log_encode(kind: SlotKind, mode: PreviewMode, result: &Result<(Vec<u8>, f64), String>) {
    let f = match result {
        Ok((dds, ms)) => {
            json!({"slot": kind.label(), "mode": format!("{mode:?}"), "ms": ms, "bytes": dds.len(), "ok": true})
        }
        Err(e) => {
            json!({"slot": kind.label(), "mode": format!("{mode:?}"), "ok": false, "error": perf_log::first_line(e)})
        }
    };
    perf_log::emit("encode", f);
}

/// The selected set's encodes, one per filled slot.
pub struct Encoder {
    pub members: BTreeMap<SlotKind, Member>,
    pub cap: u32,
    mode: PreviewMode,
    /// An export is running: nothing starts (02 §2.3).
    pub paused: bool,
    next_token: u64,
    hold: bool,
    /// A/B is in use: the user has flipped the mode once this session (kept across sets). Until then only the
    /// shown mode encodes.
    ab: bool,
    /// A member failed since the last drain: the scene must be pushed again so the game's copy of its texture
    /// comes back (a Fresh result needs no push, it is swapped in).
    pub scene_dirty: bool,
    tx: Sender<EncDone>,
    rx: Receiver<EncDone>,
}

/// A worker thread whose panic becomes `panicked(text)`: `worker::spawn_guarded` only takes `WorkerMsg`.
pub(crate) fn spawn<M: Send + 'static>(
    tx: Sender<M>,
    f: impl FnOnce() -> M + Send + 'static,
    panicked: impl FnOnce(String) -> M + Send + 'static,
) {
    std::thread::Builder::new()
        .name("tessalith-set-preview".into())
        .spawn(move || {
            let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|p| panicked(panic_text(&*p)));
            let _ = tx.send(msg);
        })
        .expect("spawn a worker thread");
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

pub(crate) fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// `v` as IEEE half bits: clamped to the f16 range, round to nearest even, NaN as 0.
pub fn f16_bits(v: f32) -> u16 {
    if v.is_nan() {
        return 0;
    }
    half::f16::from_f32(v.clamp(-65504.0, 65504.0)).to_bits()
}

/// The stored source (Note AQ) as the GPU samples `format`: (r, 0, 0, 1) for one channel, (r, g, 0, 1) for two, no
/// alpha for BC6H; so Source differs from Encoded only by the compression.
fn as_sampled(mut levels: Vec<Rgba32F>, format: Format) -> Vec<Rgba32F> {
    for f in levels.iter_mut().flat_map(|l| l.px.iter_mut()) {
        match format {
            Format::Bc4 | Format::R8 => *f = [f[0], 0.0, 0.0, 1.0],
            Format::Bc5 | Format::Rg8 => *f = [f[0], f[1], 0.0, 1.0],
            Format::Bc6hUf16 => f[3] = 1.0,
            _ => {}
        }
    }
    levels
}

/// An uncompressed DX10 DDS of `levels` (mip 0 first, tightly packed): `R8G8B8A8_UNORM[_SRGB]`, or
/// `R16G16B16A16_FLOAT` when `hdr`. The engine only writes BCn, so this is the preview's own writer.
pub fn uncompressed_dds(levels: &[Rgba32F], hdr: bool, srgb: bool) -> Result<Vec<u8>, String> {
    let first = levels.first().ok_or("no levels to write")?;
    let (w, h, mips) = (first.width, first.height, levels.len() as u32);
    let bpp: u32 = if hdr { 8 } else { 4 };
    let dxgi: u32 = match (hdr, srgb) {
        (true, _) => 10,
        (false, false) => 28,
        (false, true) => 29,
    };
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_PITCH;
    if mips > 1 {
        hd[1] |= DDSD_MIPMAPCOUNT;
    }
    (hd[2], hd[3], hd[4], hd[5], hd[6]) = (h, w, w * bpp, 1, mips);
    (hd[18], hd[19], hd[20]) = (32, DDPF_FOURCC, u32::from_le_bytes(*b"DX10"));
    hd[26] = DDSCAPS_TEXTURE
        | if mips > 1 {
            DDSCAPS_COMPLEX | DDSCAPS_MIPMAP
        } else {
            0
        };
    let mut out = b"DDS ".to_vec();
    for v in hd.into_iter().chain([dxgi, 3, 0, 1, 0]) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for l in levels {
        for c in l.px.iter().flatten() {
            if hdr {
                out.extend_from_slice(&f16_bits(*c).to_le_bytes());
            } else {
                out.push((c.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    Ok(out)
}

/// One member's bytes: in `Encoded` mode the preview encode within `cap`, made into a DDS exactly as
/// `Session::preview` builds it (`session.rs:283-290`); in `Source` mode the prepared source chain, uncompressed (no
/// BCn encode). A DDS input's own file in both.
pub fn encode(
    session: &Session,
    source: &Source,
    cap: u32,
    mode: PreviewMode,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    match source {
        Source::Job(job) if mode == PreviewMode::Source => {
            let r = session
                .preview_source_stored(job, cap, cancel)
                .map_err(|e| format!("{e:?}"))?;
            uncompressed_dds(
                &as_sampled(r.levels, job.profile.format),
                job.profile.format.is_hdr(),
                // As the Encoded file: formats without an _SRGB form (BC4/BC5/R8/RG8) are written UNORM.
                job.profile.srgb_tag()
                    && dxgi_format(job.profile.format, true)
                        != dxgi_format(job.profile.format, false),
            )
        }
        Source::Job(job) => {
            let r = session
                .preview(job, cap, cancel)
                .map_err(|e| format!("{e:?}"))?;
            let (width, height) = r.source.dims[r.source.first_level];
            let desc = DdsDesc {
                width,
                height,
                format: r.format,
                srgb: r.srgb,
                header: resolve_header(r.format, r.srgb, job.profile.header),
            };
            dds_bytes(&desc, &r.levels).map_err(|e| e.to_string())
        }
        Source::Dds(p) => std::fs::read(p).map_err(|e| format!("{}: {e}", p.display())),
        Source::Blocked(w) => Err(w.to_string()),
    }
}

fn same_bytes(a: &MemberSlot, b: &MemberSlot) -> bool {
    a.item == b.item && a.job_key == b.job_key && a.status == b.status && a.sig == b.sig
}

impl Encoder {
    pub fn new(cap: u32) -> Encoder {
        let (tx, rx) = channel();
        Encoder {
            members: BTreeMap::new(),
            cap,
            mode: PreviewMode::default(),
            paused: false,
            next_token: 0,
            hold: false,
            ab: false,
            scene_dirty: false,
            tx,
            rx,
        }
    }

    /// A new size cap: every member that has something to encode goes stale (old bytes on show), in-flight work
    /// is cancelled and its late result dropped.
    pub fn set_cap(&mut self, cap: u32, now: Instant) {
        if cap == self.cap {
            return;
        }
        self.cap = cap;
        self.restart(now, false);
    }

    pub fn mode(&self) -> PreviewMode {
        self.mode
    }

    /// A new mode. The first change puts A/B in use. A member whose other copy is Fresh just swaps (no encode, no
    /// scene push); the rest re-encode as before and keep their old bytes as the other copy. A DDS input has the same
    /// bytes in both modes, so it is left alone.
    pub fn set_mode(&mut self, mode: PreviewMode, now: Instant) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.ab = true;
        for mem in self.members.values_mut() {
            if matches!(mem.source, Source::Dds(_)) {
                mem.mode = mode;
                continue;
            }
            mem.flip(mode, self.cap, now);
        }
    }

    /// A/B is in use (the mode has been flipped this session).
    pub fn ab_active(&self) -> bool {
        self.ab
    }

    /// Test seam: `kind` has the other mode's Fresh bytes.
    #[doc(hidden)]
    pub fn has_alt(&self, kind: SlotKind) -> bool {
        self.members.get(&kind).is_some_and(|m| m.alt.is_some())
    }

    /// Test seam: the generation of `kind`'s other-mode background encode (0 = none).
    #[doc(hidden)]
    pub fn alt_token_of(&self, kind: SlotKind) -> u64 {
        self.members.get(&kind).map_or(0, |m| m.alt_token)
    }

    fn restart(&mut self, now: Instant, skip_dds: bool) {
        for mem in self.members.values_mut() {
            if skip_dds && matches!(mem.source, Source::Dds(_)) {
                mem.mode = self.mode;
                continue;
            }
            if let Some(c) = mem.cancel.take() {
                c.cancel();
            }
            mem.clear_alt();
            mem.token = 0;
            if !matches!(mem.state, EncState::Blocked(_)) {
                mem.state = EncState::Stale(mem.state.shown().cloned());
                mem.cap = self.cap;
                mem.mode = self.mode;
                mem.due = Some(now);
            }
        }
    }

    /// Test seam: the generation of `kind`'s live encode (0 = none).
    #[doc(hidden)]
    pub fn token_of(&self, kind: SlotKind) -> u64 {
        self.members.get(&kind).map_or(0, |m| m.token)
    }

    /// Test seam: while held, `pump` does its bookkeeping but starts no worker, so every completion is `inject`ed.
    #[doc(hidden)]
    pub fn hold_workers(&mut self, on: bool) {
        self.hold = on;
    }

    /// Test seam: delivers a completion as a worker would, so tests control the order.
    #[doc(hidden)]
    pub fn inject(
        &self,
        kind: SlotKind,
        token: u64,
        cap: u32,
        result: Result<(Vec<u8>, f64), String>,
    ) {
        self.inject_as(kind, token, cap, self.mode, result);
    }

    /// Test seam: [`Encoder::inject`] for a completion produced under `mode`.
    #[doc(hidden)]
    pub fn inject_as(
        &self,
        kind: SlotKind,
        token: u64,
        cap: u32,
        mode: PreviewMode,
        result: Result<(Vec<u8>, f64), String>,
    ) {
        let _ = self.tx.send(EncDone {
            kind,
            token,
            cap,
            mode,
            result,
        });
    }

    /// Another set: cancel and forget every member.
    pub fn reset(&mut self) {
        for m in self.members.values() {
            m.cancel_all();
        }
        self.members.clear();
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        for m in self.members.values() {
            m.cancel_all();
        }
    }
}

impl Encoder {
    /// The set's members now. A new member, or one whose item, status or signature changed, is encoded again: at
    /// once when `immediate` (a new selection), else after [`DEBOUNCE`] (an edit, 02 §2.4), with its old bytes still
    /// on show. A path-only change keeps the bytes. Members that left are cancelled.
    pub fn sync(&mut self, m: &AppModel, slots: Vec<MemberSlot>, now: Instant, immediate: bool) {
        let keep: Vec<SlotKind> = slots.iter().map(|s| s.kind).collect();
        self.members.retain(|k, mem| {
            let stay = keep.contains(k);
            if !stay {
                mem.cancel_all();
            }
            stay
        });
        let due = if immediate { now } else { now + DEBOUNCE };
        for slot in slots {
            let source = m
                .item(&slot.item)
                .map(source_of)
                .unwrap_or(Source::Blocked(BLOCKED_NOTHING));
            let blocked = match &source {
                Source::Blocked(w) => Some(*w),
                _ => None,
            };
            match self.members.get_mut(&slot.kind) {
                Some(mem)
                    if same_bytes(&mem.slot, &slot)
                        && mem.cap == self.cap
                        && mem.mode == self.mode =>
                {
                    mem.slot = slot
                }
                Some(mem) => {
                    if let Some(c) = mem.cancel.take() {
                        c.cancel();
                    }
                    mem.token = 0;
                    mem.clear_alt();
                    mem.cap = self.cap;
                    mem.mode = self.mode;
                    mem.state = match blocked {
                        Some(w) => EncState::Blocked(w),
                        None => EncState::Stale(mem.state.shown().cloned()),
                    };
                    mem.due = blocked.is_none().then_some(due);
                    mem.source = source;
                    mem.slot = slot;
                }
                None => {
                    let kind = slot.kind;
                    self.members.insert(
                        kind,
                        Member {
                            slot,
                            state: blocked.map_or(EncState::NotEncoded, EncState::Blocked),
                            due: blocked.is_none().then_some(due),
                            source,
                            token: 0,
                            cap: self.cap,
                            mode: self.mode,
                            cancel: None,
                            alt: None,
                            alt_token: 0,
                            alt_cancel: None,
                            alt_err: false,
                        },
                    );
                }
            }
        }
    }

    /// Starts every due encode, one thread each (a set has at most 9 members). Nothing starts while an export
    /// runs or without a session. With A/B in use and every member settled, the missing other-mode copies encode
    /// next (a DDS input has none to make).
    pub fn pump(&mut self, session: Option<&Arc<Session>>, now: Instant, export_running: bool) {
        self.paused = export_running;
        let Some(session) = session else {
            return;
        };
        if export_running {
            return;
        }
        for (kind, mem) in self.members.iter_mut() {
            if mem.due.is_none_or(|d| d > now) {
                continue;
            }
            mem.due = None;
            self.next_token += 1;
            let token = self.next_token;
            mem.token = token;
            mem.cap = self.cap;
            mem.mode = self.mode;
            let cancel = CancelToken::default();
            mem.cancel = Some(cancel.clone());
            mem.state = EncState::Encoding(mem.state.shown().cloned());
            if self.hold {
                continue;
            }
            let run = Run {
                kind: *kind,
                token,
                cap: self.cap,
                mode: self.mode,
            };
            spawn_encode(
                self.tx.clone(),
                session.clone(),
                mem.source.clone(),
                run,
                cancel,
            );
        }
        if !self.ab || !self.members.values().all(|m| m.state.settled()) {
            return;
        }
        let other = self.mode.other();
        for (kind, mem) in self.members.iter_mut() {
            if mem.alt.is_some()
                || mem.alt_token != 0
                || mem.alt_err
                || !matches!(mem.state, EncState::Fresh(_))
                || matches!(mem.source, Source::Dds(_))
            {
                continue;
            }
            self.next_token += 1;
            let token = self.next_token;
            mem.alt_token = token;
            let cancel = CancelToken::default();
            mem.alt_cancel = Some(cancel.clone());
            if self.hold {
                continue;
            }
            let run = Run {
                kind: *kind,
                token,
                cap: self.cap,
                mode: other,
            };
            spawn_encode(
                self.tx.clone(),
                session.clone(),
                mem.source.clone(),
                run,
                cancel,
            );
        }
    }

    /// Drains finished encodes. True when a member's bytes changed or it failed. An other-mode copy arriving is not
    /// a change of what is on show (the set preview sends it to the renderer on its own); a failure sets
    /// `scene_dirty`.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(EncDone {
            kind,
            token,
            cap,
            mode,
            result,
        }) = self.rx.try_recv()
        {
            let Some(mem) = self.members.get_mut(&kind) else {
                continue;
            };
            // The other mode's background encode.
            if token != 0 && mem.alt_token == token {
                log_encode(kind, mode, &result);
                mem.alt_cancel = None;
                mem.alt_token = 0;
                match result {
                    Ok((dds, ms)) if cap == self.cap && mode != self.mode => {
                        mem.alt = Some(Arc::new(Encoded {
                            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                            dds: Arc::new(dds),
                            ms,
                        }));
                    }
                    Ok(_) => {} // made at another cap or mode than now: the next pump encodes it again
                    Err(_) => mem.alt_err = true,
                }
                continue;
            }
            // Superseded: a newer edit, a block, a cap change (all zero or replace the token).
            if mem.token == 0 || mem.token != token {
                continue;
            }
            // The live encode ran at another cap or mode than the current one: useless, schedule it again.
            if cap != self.cap || mode != self.mode {
                if let Some(c) = mem.cancel.take() {
                    c.cancel();
                }
                mem.token = 0;
                mem.cap = self.cap;
                mem.mode = self.mode;
                mem.state = EncState::Stale(mem.state.shown().cloned());
                mem.due = Some(Instant::now());
                changed = true;
                continue;
            }
            log_encode(kind, mode, &result);
            mem.cancel = None;
            mem.token = 0;
            mem.state = match result {
                Ok((dds, ms)) => {
                    let e = Arc::new(Encoded {
                        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                        dds: Arc::new(dds),
                        ms,
                    });
                    // A DDS input is the same file in both modes.
                    if matches!(mem.source, Source::Dds(_)) {
                        mem.alt = Some(e.clone());
                    }
                    EncState::Fresh(e)
                }
                Err(e) => {
                    self.scene_dirty = true;
                    EncState::Failed(e)
                }
            };
            changed = true;
        }
        changed
    }

    pub fn settled(&self) -> bool {
        self.members.values().all(|m| m.state.settled())
    }

    /// Work is waiting or running (false while paused: the export keeps the app repainting). Includes the other
    /// mode's background encodes once A/B is in use.
    pub fn busy(&self) -> bool {
        !self.paused && (!self.settled() || self.alt_pending())
    }

    /// An other-mode copy is running or still to be started.
    fn alt_pending(&self) -> bool {
        self.ab
            && self.members.values().any(|m| {
                m.alt_token != 0
                    || (m.alt.is_none()
                        && !m.alt_err
                        && matches!(m.state, EncState::Fresh(_))
                        && !matches!(m.source, Source::Dds(_)))
            })
    }

    /// An encode is running (its completion is still to drain).
    fn in_flight(&self) -> bool {
        !self.paused
            && self
                .members
                .values()
                .any(|m| m.token != 0 || m.alt_token != 0)
    }

    /// Whether sdc will produce `norm` for this set (a member that can be encoded serves it): PGPatcher's
    /// existence check (01 §1.4) passes for it.
    pub fn produces(&self, norm: &str) -> bool {
        self.members.values().any(|m| {
            !matches!(m.state, EncState::Blocked(_)) && m.slot.paths.iter().any(|p| p == norm)
        })
    }

    /// Canonical path → the member's bytes on show.
    pub fn memory_sources(&self) -> HashMap<String, FileSource> {
        let mut out = HashMap::new();
        for m in self.members.values() {
            if let Some(e) = m.state.shown() {
                for p in &m.slot.paths {
                    out.insert(
                        p.clone(),
                        FileSource::Memory {
                            id: e.id,
                            bytes: e.dds.clone(),
                        },
                    );
                }
            }
        }
        out
    }
}

// ---- Part 2: fingerprint, change classification, mesh candidates, PGPatcher simulation, the panel's model ----

pub const NEEDS_CS: &str = "PGPatcher preview needs the CS target and a PBR set";
pub const JSON_WINS: &str = "Assumes your Tessalith JSON wins over other mods' PGPatcher configs.";
pub const NO_SCAN: &str = "Scan your mods in the Meshes view to preview on a real mesh";
pub const NO_MATCH: &str = "No scanned mesh uses this diffuse";
pub const NO_BUILTIN_MATCH: &str =
    "A built-in shape cannot match this entry's match keys: it shows the plain textures; pick a game mesh";
/// D4: the CS features the preview leaves off (03 §3.2 "off" rows), under the single CS toggle.
pub const LE_ONLY: &str =
    "Every mesh using this diffuse is an LE mesh, which is not drawn: showing a built-in shape";
pub const OFF_FEATURES: &str = "Off in this preview: Light Limit Fix, Skylighting, Screen-Space Shadows, \
Subsurface Scattering, Dynamic Cubemaps, Image-Based Lighting, Physical Sky, Wetness, Cloud Shadows, terrain effects, \
water, fog, Volumetric Shadows";
/// How often the fingerprint is checked when no input arrived (02 §3.2).
pub const THROTTLE: Duration = Duration::from_millis(200);
/// Rows asked of the scan DB; `nif_filter` and `shape_matches` thin them out afterwards.
pub const QUERY_LIMIT: usize = 500;
pub const BUILTINS: [(Builtin, &str); 4] = [
    (Builtin::Sphere, "Sphere"),
    (Builtin::Plane, "Plane"),
    (Builtin::Cube, "Cube"),
    (Builtin::MaterialBall, "Material ball"),
];

pub fn builtin_name(b: Builtin) -> &'static str {
    BUILTINS
        .iter()
        .find(|(x, _)| *x == b)
        .map_or("Built-in", |(_, n)| n)
}

pub fn skip_text(reason: &str) -> String {
    format!("will be skipped: {reason}")
}

pub fn badge_text(key: &str) -> String {
    format!("not previewed: {key}")
}

pub fn count_text(n: usize) -> String {
    if n == 1 {
        "1 mesh uses this diffuse".into()
    } else {
        format!("{n} meshes use this diffuse")
    }
}

/// The set the panel follows: the selected set, or the set of the selected texture (spec §8, 02 Q5).
pub fn followed_set(m: &AppModel) -> Option<String> {
    match &m.selection {
        Selection::Set(k) => Some(k.clone()),
        Selection::Item(k) => m.item(k).and_then(|i| i.set_key.clone()),
        Selection::None => None,
    }
}

/// The scan target `[meshes]` names, read as `MeshesModel::activate_with` reads it (`meshes.rs:304-311`): the
/// Meshes view may never have been opened, so its `target` may still be `None`.
pub fn target_of(s: &MeshesSettings) -> Option<ScanTarget> {
    match (&s.instance, &s.profile, &s.folder) {
        (Some(root), Some(profile), _) => Some(ScanTarget::Mo2 {
            root: root.clone(),
            profile: profile.clone(),
        }),
        (_, _, Some(f)) => Some(ScanTarget::Folder(f.clone())),
        _ => None,
    }
}

/// Everything the preview depends on, for one set (02 §3.2).
#[derive(Debug, Clone, PartialEq)]
pub struct SetFingerprint {
    pub set_key: String,
    pub label: String,
    /// The PGPatcher entry; `None` when the simulation cannot run (not CS, or not a PBR set: D3).
    pub entry: Option<EntryInput>,
    pub prefix: Option<String>,
    pub members: Vec<MemberSlot>,
}

/// `None` when the set is gone.
pub fn fingerprint(m: &AppModel, set_key: &str) -> Option<SetFingerprint> {
    let (v, entry) = match json_entries(m).into_iter().find(|(v, _)| v.key == set_key) {
        Some((v, e)) => (v, Some(e)),
        None => (set_views(m).into_iter().find(|v| v.key == set_key)?, None),
    };
    let members = member_slots(m, &v, entry.as_ref());
    Some(SetFingerprint {
        set_key: set_key.into(),
        label: v.label.clone(),
        entry,
        prefix: v.prefix.clone(),
        members,
    })
}

fn strip_ext(v: &str) -> String {
    match v.rfind('.') {
        Some(i) if !v[i..].contains(['\\', '/']) => v[..i].to_string(),
        _ => v.to_string(),
    }
}

/// The scan-DB key and `nif_filter` of an entry, read from the entry as PGPatcher reads it (`pgpsim::prepare`:
/// default merged, `texture` → `match_diffuse`). The first key set wins, in `prefix::match_mode`'s order:
/// `match_normal`, `path_contains`, `match1..8`, `match_diffuse`. `None`: no match key at all.
pub fn tex_key(entry: &Value, default: &Value) -> Option<(TexKey, Option<String>)> {
    let p = pgpsim::prepare(entry, default);
    let s = |k: &str| {
        p.get(k)
            .and_then(Value::as_str)
            .filter(|v| !v.trim_matches('\\').trim().is_empty())
            .map(str::to_string)
    };
    let stem = |v: String| strip_ext(v.trim_start_matches('\\'));
    let key = if let Some(v) = s("match_normal") {
        TexKey::Suffix {
            slot: 1,
            stem: stem(v),
        }
    } else if let Some(v) = s("path_contains") {
        TexKey::Contains { slot: 0, frag: v }
    } else if let Some((n, v)) = (1..=8u32).find_map(|n| s(&format!("match{n}")).map(|v| (n, v))) {
        TexKey::Exact {
            slot: n - 1,
            path: v,
        }
    } else {
        TexKey::Suffix {
            slot: 0,
            stem: stem(s("match_diffuse")?),
        }
    };
    Some((key, s("nif_filter")))
}

/// What a fingerprint change means (02 §3.2 table, spec §2.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// Another set, or the set switched between simulated and plain (D3): everything again.
    pub set: bool,
    /// A member's bytes change: encode it again, then reload (P1-A has no hot-swap).
    pub encode: bool,
    /// The match key or `nif_filter` changed: query the meshes again (and reload).
    pub requery: bool,
    /// Texture paths changed (prefix, slot paths): rebuild the texture map and reload.
    pub reload: bool,
    /// Only material keys changed: simulate again, `SetMaterial` for the shapes whose material changed.
    pub material: bool,
}

pub fn classify(old: Option<&SetFingerprint>, new: &SetFingerprint, default: &Value) -> Delta {
    let Some(old) =
        old.filter(|o| o.set_key == new.set_key && o.entry.is_some() == new.entry.is_some())
    else {
        return Delta {
            set: true,
            ..Default::default()
        };
    };
    let bytes = |f: &SetFingerprint| {
        f.members
            .iter()
            .map(|s| {
                (
                    s.kind,
                    s.item.clone(),
                    s.job_key.clone(),
                    s.status.clone(),
                    s.sig.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let paths = |f: &SetFingerprint| {
        f.members
            .iter()
            .map(|s| (s.kind, s.paths.clone()))
            .collect::<Vec<_>>()
    };
    let mut d = Delta {
        encode: bytes(old) != bytes(new),
        reload: paths(old) != paths(new) || old.prefix != new.prefix,
        ..Default::default()
    };
    if let (Some(a), Some(b)) = (&old.entry, &new.entry) {
        let (va, vb) = (entry_node(a).to_value(), entry_node(b).to_value());
        if tex_key(&va, default) != tex_key(&vb, default) {
            d.requery = true;
            d.reload = true;
        }
        if a.slots != b.slots {
            d.reload = true;
        }
        d.material = va != vb && !d.reload;
    }
    d
}

/// One game NIF whose shapes match the entry (02 §5.3), best first.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub load_copy_id: i64,
    /// The game winner (another copy when PGPatcher's output wins in game).
    pub copy_id: i64,
    /// PGPatcher form, as `pgpsim` takes it: `meshes\…`.
    pub nif_path: String,
    pub origin: String,
    pub le_mesh: bool,
    pub derived: bool,
    /// (block, name) of the matching shapes, re-checked with `pgpsim::shape_matches`.
    pub shapes: Vec<(u32, Option<String>)>,
}

impl Candidate {
    pub fn label(&self) -> String {
        format!(
            "{} · {} · {} matching shape{}{}{}",
            self.nif_path,
            self.origin,
            self.shapes.len(),
            if self.shapes.len() == 1 { "" } else { "s" },
            if self.le_mesh {
                " · LE, not drawn"
            } else {
                ""
            },
            if self.derived {
                " · patched by PGPatcher in game"
            } else {
                ""
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Candidates {
    NotAsked,
    NoStore(String),
    Querying,
    Ready(Vec<Candidate>),
    Error(String),
}

/// Rows → one candidate per copy to load, in row order. `filter` is `nif_filter` (`icontains` on the `meshes\…`
/// path, 01 §1.3); `check(load copy, nif path, block)` is the `shape_matches` re-check.
pub fn group_rows(
    rows: &[MatchRow],
    filter: Option<&str>,
    check: &dyn Fn(i64, &str, u32) -> bool,
) -> Vec<Candidate> {
    let filter = filter.map(str::to_ascii_lowercase);
    let mut out: Vec<Candidate> = vec![];
    for r in rows {
        let nif = r.nif_path.replace('/', "\\");
        if filter
            .as_deref()
            .is_some_and(|f| !nif.to_ascii_lowercase().contains(f))
        {
            continue;
        }
        if !check(r.load_copy_id, &nif, r.shape_block) {
            continue;
        }
        match out.iter_mut().find(|c| c.load_copy_id == r.load_copy_id) {
            Some(c) => c.shapes.push((r.shape_block, r.shape_name.clone())),
            None => out.push(Candidate {
                load_copy_id: r.load_copy_id,
                copy_id: r.copy_id,
                nif_path: nif,
                origin: r.origin.clone(),
                le_mesh: r.le_mesh,
                derived: r.derived,
                shapes: vec![(r.shape_block, r.shape_name.clone())],
            }),
        }
    }
    out
}

/// A stored shape's slots as PGPatcher reads them: the raw string when the store kept it, else the canonical path
/// with backslashes (the same string up to case, which PGPatcher folds).
fn view_slots(v: &ShapeView) -> Vec<TexPath> {
    v.textures
        .iter()
        .zip(&v.raw)
        .map(|(norm, raw)| match (raw, norm) {
            (Some(r), _) => nif::path::canon_texture(r.raw.as_bytes()),
            (None, Some(n)) => nif::path::canon_texture(n.replace('/', "\\").as_bytes()),
            (None, None) => nif::path::canon_texture(b""),
        })
        .collect()
}

/// The worker side of a query: its own read-only connection (a `Connection` is not `Sync`; a scan may be writing).
fn query(
    path: &Path,
    key: &TexKey,
    filter: Option<&str>,
    prepared: &Value,
) -> Result<Vec<Candidate>, String> {
    let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
    let rows = store
        .nifs_using_texture(key, QUERY_LIMIT)
        .map_err(|e| e.to_string())?;
    let cache: RefCell<HashMap<i64, Vec<ShapeView>>> = RefCell::new(HashMap::new());
    let failed: RefCell<Option<String>> = RefCell::new(None);
    let check = |copy: i64, nif: &str, block: u32| -> bool {
        let mut c = cache.borrow_mut();
        let shapes = c.entry(copy).or_insert_with(|| {
            store.shapes_of(copy).unwrap_or_else(|e| {
                *failed.borrow_mut() = Some(e.to_string());
                vec![]
            })
        });
        shapes
            .iter()
            .find(|s| s.block == block)
            .is_some_and(|s| pgpsim::shape_matches(prepared, nif, &view_slots(s)))
    };
    let out = group_rows(&rows, filter, &check);
    if let Some(e) = failed.into_inner() {
        return Err(format!("reading the shapes of a mesh: {e}"));
    }
    Ok(out)
}

/// A game NIF read and parsed for the preview: source materials, the game's copy of every texture its shapes name,
/// and its path in PGPatcher form.
#[derive(Debug, Clone)]
pub struct LoadedNif {
    pub load_copy_id: i64,
    pub nif_path: String,
    pub parsed: ParsedNif,
    pub vfs: HashMap<String, FileSource>,
    pub ms: f64,
}

fn load_nif(path: &Path, c: &Candidate) -> Result<LoadedNif, String> {
    let t0 = Instant::now();
    let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
    let (p, kind, root, _) = store
        .copy_location(c.load_copy_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{}: no longer in the scan", c.nif_path))?;
    let bytes = read_file(&FileSource::from_store(kind, &root, &p))?;
    let parsed = render3d::parse(&bytes);
    let mut vfs = HashMap::new();
    for t in parsed.shapes.iter().flat_map(|s| s.material.slots.iter()) {
        if t.norm.is_empty() || vfs.contains_key(&t.norm) {
            continue;
        }
        if let Some((k, r)) = store.texture_location(&t.norm).map_err(|e| e.to_string())? {
            vfs.insert(t.norm.clone(), FileSource::from_store(k, &r, &t.norm));
        }
    }
    Ok(LoadedNif {
        load_copy_id: c.load_copy_id,
        nif_path: c.nif_path.clone(),
        parsed,
        vfs,
        ms: ms(t0),
    })
}

/// What PGPatcher does to one shape of the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Keys the preview cannot show (`smooth_angle`, `auto_uv`, …): the "not previewed" badges.
    Patched {
        badges: Vec<&'static str>,
    },
    /// PGPatcher would skip the shape for this file (01 §1.4); the shape keeps its source material (red badge).
    Skip(String),
    NoMatch,
    /// No simulation (D3, or a built-in shape no slot of which can match the entry).
    Plain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRow {
    pub id: ShapeId,
    pub name: String,
    pub outcome: Outcome,
    /// What the renderer draws: the patched material, or the source one.
    pub material: MaterialState,
}

impl ShapeRow {
    pub fn text(&self) -> String {
        match &self.outcome {
            Outcome::Patched { .. } => format!("{}: patched as PGPatcher would", self.name),
            Outcome::Skip(r) => format!("{}: {}", self.name, skip_text(r)),
            Outcome::NoMatch => format!("{}: not matched, original textures", self.name),
            Outcome::Plain => format!("{}: plain textures, no PGPatcher simulation", self.name),
        }
    }
}

/// sdc's one entry on each shape (`pgpsim::simulate`). `exists` gets PGPatcher-form paths.
pub fn simulate_shapes(
    entry: &Value,
    default: &Value,
    nif_path: &str,
    shapes: &[(ShapeId, String, MaterialState)],
    exists: &dyn Fn(&str) -> bool,
) -> Vec<ShapeRow> {
    shapes
        .iter()
        .map(|(id, name, source)| {
            let r = pgpsim::simulate(&SimInput {
                entry,
                default,
                nif_path,
                source,
                exists,
            });
            let (outcome, material) = match r {
                SimResult::Patched {
                    material, badges, ..
                } => (
                    Outcome::Patched {
                        badges: badges
                            .iter()
                            .map(|b| match b {
                                Badge::NotPreviewed(k) => *k,
                            })
                            .collect(),
                    },
                    material,
                ),
                SimResult::Skip(why) => (Outcome::Skip(why), source.clone()),
                SimResult::NoMatch => (Outcome::NoMatch, source.clone()),
            };
            ShapeRow {
                id: *id,
                name: name.clone(),
                outcome,
                material,
            }
        })
        .collect()
}

/// A built-in shape as a shape the entry matches: `builtin_default` with the slot its match key reads set to a
/// path that key matches, and a mesh path that passes `nif_filter`. `None` when no such stand-in matches.
pub fn builtin_source(entry: &Value, default: &Value) -> Option<(MaterialState, String)> {
    let (key, filter) = tex_key(entry, default)?;
    let mut m = MaterialState::builtin_default();
    match &key {
        TexKey::Suffix { slot: 0, stem } => m.set_slot_raw(0, &format!("textures\\{stem}.dds")),
        TexKey::Suffix { slot, stem } => {
            m.set_slot_raw(*slot as usize, &format!("textures\\{stem}_n.dds"))
        }
        TexKey::Contains { slot, frag } => m.set_slot_raw(
            *slot as usize,
            &format!("textures\\{}.dds", frag.trim_matches('\\')),
        ),
        TexKey::Exact { slot, path } => {
            let p = if path.to_ascii_lowercase().starts_with("textures\\") {
                path.clone()
            } else {
                format!("textures\\{path}")
            };
            m.set_slot_raw(*slot as usize, &p)
        }
    }
    let nif = match &filter {
        Some(f) => format!("meshes\\{}\\sdcbuiltin.nif", f.trim_matches('\\')),
        None => "meshes\\sdcbuiltin.nif".to_string(),
    };
    let prepared = pgpsim::prepare(entry, default);
    pgpsim::shape_matches(&prepared, &nif, &m.slots).then_some((m, nif))
}

/// No simulation: `builtin_default` with each member's first path at its vanilla slot (diffuse 0, normal 1,
/// glow/skin 2, height 3).
pub fn plain_material(members: &[MemberSlot]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for s in members {
        let slot = match s.kind {
            SlotKind::Diffuse => 0,
            SlotKind::Normal => 1,
            SlotKind::Emissive | SlotKind::SkinSk => 2,
            SlotKind::Height => 3,
            _ => continue,
        };
        if let Some(p) = s.paths.first() {
            m.set_slot_raw(slot, &p.replace('/', "\\"));
        }
    }
    m
}

/// The mesh per set (spec §2.3): the best candidate, a picked NIF, or a built-in shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshChoice {
    Auto,
    Nif(i64),
    Builtin(Builtin),
}

/// What is drawn now.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Nif(Candidate),
    Builtin(Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Nif(i64),
    Builtin(Builtin),
}

/// Wall times for the verification (Task 17); encode times are on each member, edit times in `v3.last_edit`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timings {
    pub query_ms: Option<f64>,
    pub nif_ms: Option<f64>,
    /// Simulation of every shape plus building the scene.
    pub push_ms: Option<f64>,
}

enum SpMsg {
    Queried {
        token: u64,
        result: Result<(Vec<Candidate>, f64), String>,
    },
    NifLoaded {
        token: u64,
        result: Result<LoadedNif, String>,
    },
}

pub struct SetPreview {
    /// Its own viewport (D2: a second D3D device; the shader cache on disk is shared).
    pub v3: Viewport3d,
    pub enc: Encoder,
    /// Off in `SetPreview::default()` (so `ui::draw`'s test state keeps the existing layout); on from `new`.
    pub enabled: bool,
    pub store_path: Option<PathBuf>,
    /// What the CS locate searches.
    pub target: Option<ScanTarget>,
    pub fp: Option<SetFingerprint>,
    pub candidates: Candidates,
    /// The mesh per set key, this session only (spec §8).
    pub choice: HashMap<String, MeshChoice>,
    pub rows: Vec<ShapeRow>,
    pub notices: Vec<String>,
    /// The NIF read's own notices (warnings, a failed read): dropped when the mesh pick changes.
    nif_notices: Vec<String>,
    pub timings: Timings,
    /// Set by the panel each frame it draws; `poll` does nothing heavy otherwise.
    pub shown: bool,
    /// Set by the panel on a frame with input: the fingerprint is checked now instead of after `THROTTLE`.
    pub input: bool,
    /// Fingerprint evaluations so far (tests assert an unchanged frame within `THROTTLE` does none).
    pub checks: u32,
    default: Value,
    gone: Option<String>,
    vfs_err: RefCell<Option<String>>,
    entry: Option<Value>,
    nif: Option<Arc<LoadedNif>>,
    failed: HashSet<i64>,
    on_screen: Option<Shown>,
    sent: BTreeMap<ShapeId, MaterialState>,
    /// Every slot path the last pushed scene's materials named (resolved or not): a JSON-only edit naming another
    /// one needs the push, which runs the VFS lookup.
    pushed_norms: HashSet<String>,
    /// What the viewport's renderer holds of the encoder's bytes: (path, mode) -> `Encoded.id`, and the mode each path
    /// is showing. Rebuilt by every scene push, kept in line by [`SetPreview::sync_textures`] (P1-B A/B).
    tex_sent: HashMap<(String, PreviewMode), u64>,
    tex_active: HashMap<String, PreviewMode>,
    want_scene: bool,
    /// The panel was drawn on the last polled frame: a pending scene push only counts as work while it is on screen.
    visible: bool,
    querying: Option<u64>,
    loading: Option<(u64, i64)>,
    last_check: Option<Instant>,
    located: bool,
    next: u64,
    store: RefCell<Option<Store>>,
    store_tried: Cell<bool>,
    vfs_cache: RefCell<HashMap<String, Option<FileSource>>>,
    tx: Sender<SpMsg>,
    rx: Receiver<SpMsg>,
}

impl Default for SetPreview {
    /// No IO and no threads: no store path, no saved shaders, disabled.
    fn default() -> SetPreview {
        let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
        s.enabled = false;
        s
    }
}

impl SetPreview {
    /// `shaders`: `[render3d] shaders` from `gui.toml`; `store_path`: the scan DB. Opens nothing until shown.
    pub fn new(
        shaders: Option<render3d::locate::Fingerprint>,
        target: Option<ScanTarget>,
        store_path: Option<PathBuf>,
        cap: u32,
    ) -> SetPreview {
        let (tx, rx) = channel();
        SetPreview {
            v3: {
                let mut v = Viewport3d::new(shaders);
                v.panel = "set_preview";
                v
            },
            enc: Encoder::new(cap),
            enabled: true,
            store_path,
            target,
            fp: None,
            candidates: Candidates::NotAsked,
            choice: HashMap::new(),
            rows: vec![],
            notices: vec![],
            nif_notices: vec![],
            timings: Timings::default(),
            shown: false,
            input: false,
            checks: 0,
            gone: None,
            vfs_err: RefCell::new(None),
            default: document(&[]).to_value()["default"].clone(),
            entry: None,
            nif: None,
            failed: HashSet::new(),
            on_screen: None,
            sent: BTreeMap::new(),
            pushed_norms: HashSet::new(),
            tex_sent: HashMap::new(),
            tex_active: HashMap::new(),
            want_scene: false,
            visible: false,
            querying: None,
            loading: None,
            last_check: None,
            located: false,
            next: 0,
            store: RefCell::new(None),
            store_tried: Cell::new(false),
            vfs_cache: RefCell::new(HashMap::new()),
            tx,
            rx,
        }
    }

    pub fn mode(&self) -> PreviewMode {
        self.enc.mode()
    }

    /// Encoded / Source (not persisted): only the encodes change, the meshes stay. With both variants encoded this
    /// is a `SetTextureVariant` per path and nothing else; otherwise the new mode encodes as before and its bytes
    /// are swapped in when they land. The viewport cannot take it yet (no load of this scene sent): push the scene.
    pub fn set_mode(&mut self, mode: PreviewMode) {
        if mode == self.enc.mode() {
            return;
        }
        self.enc.set_mode(mode, Instant::now());
        if !self.sync_textures() {
            self.want_scene = true;
        }
    }

    /// Test seam: a query and a NIF-load completion carrying `token`, as a worker would send them.
    #[doc(hidden)]
    pub fn inject_stale(&self, token: u64, error: &str) {
        let _ = self.tx.send(SpMsg::Queried {
            token,
            result: Err(error.into()),
        });
        let _ = self.tx.send(SpMsg::NifLoaded {
            token,
            result: Err(error.into()),
        });
    }

    /// Encodes, queries, NIF reads, the viewport or a scene to send are in flight.
    pub fn busy(&self) -> bool {
        // Hidden, `pump` never runs: waiting (debounced) members are not work until the panel is shown again.
        self.v3.busy()
            || (if self.shown || self.visible {
                self.enc.busy()
            } else {
                self.enc.in_flight()
            })
            || self.querying.is_some()
            || self.loading.is_some()
            || (self.want_scene && !self.enc.paused && (self.shown || self.visible))
    }

    pub fn poll(&mut self, m: &AppModel) {
        self.poll_at(m, Instant::now())
    }

    /// Window focus (`app.rs`): look for CS and re-read its settings again, once this panel has located it.
    pub fn relocate(&mut self) {
        if self.located {
            self.v3.locate(self.target.clone());
        }
    }

    /// `poll` at a given time (tests drive the debounce and the throttle). Call once per frame.
    pub fn poll_at(&mut self, m: &AppModel, now: Instant) {
        self.v3.poll_at(now);
        // A finished encode on a scene that is already loaded is swapped in (`sync_textures`); the scene is pushed
        // for a first load and when an encode failed (the game's copy of the texture must come back).
        let changed = self.enc.poll();
        let failed = std::mem::take(&mut self.enc.scene_dirty);
        if changed && (failed || self.on_screen.is_none()) {
            self.want_scene = true;
        }
        self.drain();
        let shown = std::mem::take(&mut self.shown);
        let input = std::mem::take(&mut self.input);
        self.visible = shown;
        if !shown || !self.enabled {
            return;
        }
        if !self.located {
            self.located = true;
            self.v3.locate(self.target.clone());
        }
        let key = followed_set(m);
        let due = input
            || (key.as_ref() != self.fp.as_ref().map(|f| &f.set_key)
                && key.as_ref() != self.gone.as_ref())
            || self
                .last_check
                .is_none_or(|t| now.duration_since(t) >= THROTTLE);
        if due {
            self.last_check = Some(now);
            if let Some(k) = key {
                self.check(m, &k, now);
            }
        }
        self.enc
            .pump(m.session.as_ref(), now, m.export.running.is_some());
        if self.want_scene && self.enc.settled() && self.querying.is_none() {
            self.push_scene();
        }
        if !self.want_scene && !self.sync_textures() {
            self.want_scene = true;
        }
        for e in std::mem::take(&mut self.v3.swap_errors) {
            self.notice(e);
        }
        let e = self.vfs_err.borrow().clone();
        if let Some(e) = e {
            self.notice(e);
        }
    }

    fn nif_notice(&mut self, n: String) {
        self.nif_notices.push(n.clone());
        self.notice(n);
    }

    fn drop_nif_notices(&mut self) {
        let old = std::mem::take(&mut self.nif_notices);
        self.notices.retain(|n| !old.contains(n));
    }

    fn notice(&mut self, n: String) {
        if !self.notices.contains(&n) {
            self.notices.push(n);
        }
    }

    fn check(&mut self, m: &AppModel, key: &str, now: Instant) {
        self.checks += 1;
        let Some(fp) = fingerprint(m, key) else {
            // The followed set is gone: drop the scene and remember the key so the next frames settle.
            if self.fp.is_some() {
                self.enc.reset();
                self.rows.clear();
                self.notices.clear();
                self.nif = None;
                self.sent.clear();
                self.entry = None;
                self.fp = None;
                self.candidates = Candidates::NotAsked;
                self.querying = None;
                self.loading = None;
                self.on_screen = None;
                self.want_scene = false;
                self.v3.set_content(None, false);
            }
            self.gone = Some(key.to_string());
            return;
        };
        self.gone = None;
        let d = classify(self.fp.as_ref(), &fp, &self.default);
        if d == Delta::default() {
            return;
        }
        if d.set {
            self.enc.reset();
            self.rows.clear();
            self.notices.clear();
            self.nif_notices.clear();
            self.nif = None;
            self.failed.clear();
            self.sent.clear();
            self.candidates = Candidates::NotAsked;
            self.querying = None;
            self.loading = None;
            self.on_screen = None;
        }
        self.entry = fp.entry.as_ref().map(|e| entry_node(e).to_value());
        self.enc.sync(m, fp.members.clone(), now, d.set);
        self.fp = Some(fp);
        if d.set || d.requery {
            self.requery();
        }
        if d.set || d.encode || d.reload {
            self.want_scene = true;
        } else if d.material {
            self.apply_materials();
        }
    }

    fn requery(&mut self) {
        // A scan may have run since: reopen lazily, forget cached lookups.
        self.store_tried.set(false);
        *self.store.borrow_mut() = None;
        self.vfs_cache.borrow_mut().clear();
        *self.vfs_err.borrow_mut() = None;
        self.nif = None;
        self.failed.clear();
        self.querying = None;
        self.notices.retain(|n| !n.starts_with("scan database: "));
        let Some(entry) = self.entry.clone() else {
            self.candidates = Candidates::NotAsked;
            return;
        };
        let Some(path) = self.store_path.clone().filter(|p| p.is_file()) else {
            self.candidates = Candidates::NoStore(NO_SCAN.into());
            return;
        };
        let Some((key, filter)) = tex_key(&entry, &self.default) else {
            self.candidates = Candidates::Ready(vec![]);
            return;
        };
        self.next += 1;
        let token = self.next;
        self.querying = Some(token);
        self.candidates = Candidates::Querying;
        let prepared = pgpsim::prepare(&entry, &self.default);
        spawn(
            self.tx.clone(),
            move || {
                let t0 = Instant::now();
                let result = query(&path, &key, filter.as_deref(), &prepared).map(|c| (c, ms(t0)));
                SpMsg::Queried { token, result }
            },
            move |e| SpMsg::Queried {
                token,
                result: Err(format!("internal error in the mesh query: {e}")),
            },
        );
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                SpMsg::Queried { token, result } if self.querying == Some(token) => {
                    self.querying = None;
                    match result {
                        Ok((c, t)) => {
                            self.timings.query_ms = Some(t);
                            self.candidates = Candidates::Ready(c);
                        }
                        Err(e) => self.candidates = Candidates::Error(e),
                    }
                    self.want_scene = true;
                }
                SpMsg::NifLoaded { token, result }
                    if self.loading.is_some_and(|(t, _)| t == token) =>
                {
                    let (_, copy) = self.loading.take().expect("checked above");
                    self.drop_nif_notices();
                    match result {
                        Ok(n) => {
                            self.timings.nif_ms = Some(n.ms);
                            for e in n.parsed.errors.iter().chain(&n.parsed.warnings) {
                                self.nif_notice(format!("{}: {e}", n.nif_path));
                            }
                            self.nif = Some(Arc::new(n));
                        }
                        Err(e) => {
                            self.failed.insert(copy);
                            self.nif_notice(e);
                        }
                    }
                    self.want_scene = true;
                }
                _ => {}
            }
        }
    }

    /// The mesh drawn now: the set's choice when usable, else the best usable candidate, else the sphere.
    pub fn pick(&self) -> Pick {
        let Some(fp) = &self.fp else {
            return Pick::Builtin(Builtin::Sphere);
        };
        let choice = self
            .choice
            .get(&fp.set_key)
            .copied()
            .unwrap_or(MeshChoice::Auto);
        let list: &[Candidate] = match (&self.candidates, fp.entry.is_some()) {
            (Candidates::Ready(v), true) => v,
            _ => &[],
        };
        let usable = |c: &&Candidate| !c.le_mesh && !self.failed.contains(&c.load_copy_id);
        let best = || list.iter().find(usable);
        let found = match choice {
            MeshChoice::Builtin(b) => return Pick::Builtin(b),
            MeshChoice::Nif(id) => list
                .iter()
                .filter(usable)
                .find(|c| c.load_copy_id == id)
                .or_else(best),
            MeshChoice::Auto => best(),
        };
        found.map_or(Pick::Builtin(Builtin::Sphere), |c| Pick::Nif(c.clone()))
    }

    /// The user's mesh choice for the followed set (`Auto` when none).
    pub fn choice(&self) -> MeshChoice {
        self.fp
            .as_ref()
            .and_then(|fp| self.choice.get(&fp.set_key).copied())
            .unwrap_or(MeshChoice::Auto)
    }

    pub fn choose(&mut self, c: MeshChoice) {
        if let Some(fp) = &self.fp {
            self.choice.insert(fp.set_key.clone(), c);
            self.drop_nif_notices();
            self.want_scene = true;
        }
    }

    /// The game's copy of `norm` (UI-thread read-only connection, opened on first use; lookups cached).
    fn vfs_lookup(&self, norm: &str) -> Option<FileSource> {
        if let Some(hit) = self.vfs_cache.borrow().get(norm) {
            return hit.clone();
        }
        if self.vfs_err.borrow().is_some() {
            return None;
        }
        if !self.store_tried.get() {
            self.store_tried.set(true);
            if let Some(p) = self.store_path.as_ref().filter(|p| p.is_file()) {
                match Store::open_read_only(p) {
                    Ok(st) => *self.store.borrow_mut() = Some(st),
                    Err(e) => {
                        *self.vfs_err.borrow_mut() = Some(format!("scan database: {e}"));
                        return None;
                    }
                }
            }
        }
        let found = match self
            .store
            .borrow()
            .as_ref()
            .map(|s| s.texture_location(norm))
        {
            Some(Err(e)) => {
                *self.vfs_err.borrow_mut() = Some(format!("scan database: {e}"));
                return None;
            }
            Some(Ok(f)) => f.map(|(k, r)| FileSource::from_store(k, &r, norm)),
            None => None,
        };
        self.vfs_cache
            .borrow_mut()
            .insert(norm.to_string(), found.clone());
        found
    }

    /// PGPatcher's existence check (01 §1.4, TP:348-375). pgpsim calls it for every non-empty slot after its slot
    /// pass, including untouched vanilla slots, `slotN` paths and the cubemap, in PGPatcher form (backslashes, lower
    /// case). True when sdc produces the path (a member that can be encoded) or the game has a winning copy of it,
    /// loose or in a BSA (`Store::texture_location` over `tex_winner`, which holds BSA origins too). Without this,
    /// a shape that keeps a vanilla slot would come back as `Skip`.
    pub fn exists(&self, pgp_path: &str) -> bool {
        let n = canon(pgp_path);
        // Unknown (a store error) counts as existing: an error must never show a false "will be skipped".
        self.enc.produces(&n) || self.vfs_lookup(&n).is_some() || self.vfs_err.borrow().is_some()
    }

    /// Adds the game's copy of every slot texture of `materials` that `map` lacks (a path PGPatcher wrote that the
    /// source NIF did not name: another mod's `_rmaos`, a `slotN`).
    fn add_vfs(&self, map: &mut HashMap<String, FileSource>, materials: &[&MaterialState]) {
        for t in materials.iter().flat_map(|m| m.slots.iter()) {
            if t.norm.is_empty() || map.contains_key(&t.norm) {
                continue;
            }
            if let Some(src) = self.vfs_lookup(&t.norm) {
                map.insert(t.norm.clone(), src);
            }
        }
        // The skin RFAOS candidates too (render3d resolves only through this map), optional: a failed read is quiet.
        for m in materials {
            let slot = |i: usize| m.slots.get(i).map_or("", |p| p.norm.as_str());
            for c in render3d::textures::rfaos_candidates(m.shader_type, slot(1), slot(7)) {
                if map.contains_key(&c) {
                    continue;
                }
                if let Some(src) = self.vfs_lookup(&c) {
                    map.insert(c, FileSource::Optional(Box::new(src)));
                }
            }
        }
    }

    /// The built-in shape's material and row: the entry simulated on a matching stand-in, or the plain textures.
    fn builtin_scene(&mut self) -> (MaterialState, ShapeRow) {
        let name = "Built-in shape".to_string();
        if let Some(entry) = self.entry.clone() {
            if let Some((src, nif)) = builtin_source(&entry, &self.default) {
                let mut rows = simulate_shapes(
                    &entry,
                    &self.default,
                    &nif,
                    &[(ShapeId(0), name, src)],
                    &|p| self.exists(p),
                );
                let row = rows.remove(0);
                return (row.material.clone(), row);
            }
            if !self.notices.iter().any(|n| n == NO_BUILTIN_MATCH) {
                self.notices.push(NO_BUILTIN_MATCH.into());
            }
        }
        let members = self
            .fp
            .as_ref()
            .map(|f| f.members.clone())
            .unwrap_or_default();
        let m = plain_material(&members);
        (
            m.clone(),
            ShapeRow {
                id: ShapeId(0),
                name,
                outcome: Outcome::Plain,
                material: m,
            },
        )
    }

    fn nif_rows(&self, nif: &LoadedNif, entry: &Value) -> Vec<ShapeRow> {
        let shapes: Vec<(ShapeId, String, MaterialState)> = nif
            .parsed
            .shapes
            .iter()
            .map(|s| (s.id, s.name.clone(), s.material.clone()))
            .collect();
        simulate_shapes(entry, &self.default, &nif.nif_path, &shapes, &|p| {
            self.exists(p)
        })
    }

    /// Builds the scene and hands it to the viewport: every shape simulated, its material written into the parsed
    /// NIF before `LoadParsed` (no first-load edit burst), textures = the game's copies overlaid by the set's
    /// encodes. A NIF not read yet is read first (off the UI thread); the push repeats when it arrives.
    fn push_scene(&mut self) {
        let t0 = Instant::now();
        self.want_scene = false;
        let memory = self.enc.memory_sources();
        self.v3.set_load_variant(variant_of(self.enc.mode()));
        match self.pick() {
            Pick::Builtin(b) => {
                if let (Candidates::Ready(l), Some(fp)) = (&self.candidates, &self.fp) {
                    let auto =
                        !matches!(self.choice.get(&fp.set_key), Some(MeshChoice::Builtin(_)));
                    if auto && !l.is_empty() && l.iter().all(|c| c.le_mesh) {
                        self.notice(LE_ONLY.into());
                    }
                }
                let (material, row) = self.builtin_scene();
                let mut textures = memory;
                self.add_vfs(&mut textures, &[&material]);
                let same = self.on_screen == Some(Shown::Builtin(b));
                self.next += 1;
                self.v3.set_content(
                    Some(Content::Builtin {
                        key: self.next,
                        shape: b,
                        material: material.clone(),
                        textures: Arc::new(textures),
                    }),
                    same,
                );
                self.on_screen = Some(Shown::Builtin(b));
                self.pushed_norms = slot_norms(&[&material]);
                self.sent = BTreeMap::from([(ShapeId(0), material)]);
                self.rows = vec![row];
            }
            Pick::Nif(c) => {
                let Some(nif) = self
                    .nif
                    .clone()
                    .filter(|n| n.load_copy_id == c.load_copy_id)
                else {
                    self.start_nif(&c);
                    return;
                };
                let Some(entry) = self.entry.clone() else {
                    return;
                };
                let rows = self.nif_rows(&nif, &entry);
                let mut parsed = nif.parsed.clone();
                for (s, r) in parsed.shapes.iter_mut().zip(&rows) {
                    s.material = r.material.clone();
                }
                let mut textures = nif.vfs.clone();
                self.add_vfs(
                    &mut textures,
                    &rows.iter().map(|r| &r.material).collect::<Vec<_>>(),
                );
                textures.extend(memory);
                let same = self.on_screen == Some(Shown::Nif(c.load_copy_id));
                self.next += 1;
                self.v3.set_content(
                    Some(Content::Parsed {
                        key: self.next,
                        nif: Arc::new(parsed),
                        textures: Arc::new(textures),
                    }),
                    same,
                );
                self.on_screen = Some(Shown::Nif(c.load_copy_id));
                self.pushed_norms =
                    slot_norms(&rows.iter().map(|r| &r.material).collect::<Vec<_>>());
                self.sent = rows.iter().map(|r| (r.id, r.material.clone())).collect();
                self.rows = rows;
            }
        }
        self.reset_tex_mirror();
        self.timings.push_ms = Some(ms(t0));
    }

    fn start_nif(&mut self, c: &Candidate) {
        if self.loading.is_some_and(|(_, id)| id == c.load_copy_id) {
            return;
        }
        let Some(path) = self.store_path.clone() else {
            return;
        };
        self.next += 1;
        let token = self.next;
        self.loading = Some((token, c.load_copy_id));
        let c = c.clone();
        spawn(
            self.tx.clone(),
            move || SpMsg::NifLoaded {
                token,
                result: load_nif(&path, &c),
            },
            move |e| SpMsg::NifLoaded {
                token,
                result: Err(format!("internal error reading the NIF: {e}")),
            },
        );
    }

    /// After a scene push the renderer holds exactly what `memory_sources` served, as the current mode's variant
    /// (`set_load_variant`), and shows it.
    fn reset_tex_mirror(&mut self) {
        self.tex_sent.clear();
        self.tex_active.clear();
        let mode = self.enc.mode();
        for m in self.enc.members.values() {
            for p in &m.slot.paths {
                // A path without bytes on show may still be loaded from the game's copy, under the same label.
                self.tex_active.insert(p.clone(), mode);
                if let Some(e) = m.state.shown() {
                    self.tex_sent.insert((p.clone(), mode), e.id);
                }
            }
        }
    }

    /// Brings the renderer's copy of the set's textures in line with the encoder without a scene push (P1-B Task 8):
    /// the shown mode's Fresh bytes as `SwapTexture`; once A/B is in use the other mode's Fresh bytes beside them
    /// (only for a path the renderer already shows: a lone copy would become the shown one) and an empty
    /// `SwapTexture` for a copy that went stale; then `SetTextureVariant` for the shown mode. False when the viewport
    /// cannot take a required command (no load of this scene has been sent yet): the caller pushes the scene.
    fn sync_textures(&mut self) -> bool {
        if self.on_screen.is_none() {
            return true;
        }
        let mode = self.enc.mode();
        let other = mode.other();
        let ab = self.enc.ab_active();
        // Later members win a shared path, as in `memory_sources`.
        let mut want: HashMap<(String, PreviewMode), Arc<Encoded>> = HashMap::new();
        let mut paths: Vec<String> = vec![];
        let mut stale: Vec<(String, PreviewMode)> = vec![];
        for m in self.enc.members.values() {
            for p in &m.slot.paths {
                paths.push(p.clone());
                if let EncState::Fresh(e) = &m.state {
                    want.insert((p.clone(), mode), e.clone());
                }
                match (&m.alt, ab) {
                    (Some(e), true) => {
                        want.insert((p.clone(), other), e.clone());
                    }
                    // A stale copy goes, unless it is the one on screen (a flip before the new mode landed keeps
                    // showing the old picture until it does).
                    _ if self.tex_sent.contains_key(&(p.clone(), other))
                        && self.tex_active.get(p) != Some(&other) =>
                    {
                        stale.push((p.clone(), other))
                    }
                    _ => {}
                }
            }
        }
        // Another member sharing the path may hold the copy: never drop what is also wanted.
        stale.retain(|k| !want.contains_key(k));
        let mut flip: HashSet<String> = HashSet::new();
        for ((p, label), e) in want.iter().filter(|(k, _)| k.1 == mode) {
            if self.tex_sent.get(&(p.clone(), *label)) == Some(&e.id) {
                continue;
            }
            let is_new = !self.tex_sent.contains_key(&(p.clone(), *label));
            if !self
                .v3
                .swap_texture(p.clone(), e.dds.clone(), variant_of(*label))
            {
                return false;
            }
            self.tex_sent.insert((p.clone(), *label), e.id);
            // The renderer shows the first texture a path gets.
            self.tex_active.entry(p.clone()).or_insert(*label);
            if is_new {
                flip.insert(p.clone());
            }
        }
        for ((p, label), e) in want.iter().filter(|(k, _)| k.1 == other) {
            if !self.tex_sent.contains_key(&(p.clone(), mode))
                || self.tex_sent.get(&(p.clone(), *label)) == Some(&e.id)
            {
                continue;
            }
            // Not required: refused now, tried again on the next frame.
            if self
                .v3
                .swap_texture(p.clone(), e.dds.clone(), variant_of(*label))
            {
                self.tex_sent.insert((p.clone(), *label), e.id);
            }
        }
        for key in stale {
            if self
                .v3
                .swap_texture(key.0.clone(), Arc::new(vec![]), variant_of(key.1))
            {
                self.tex_sent.remove(&key);
            }
        }
        for p in paths {
            if !self.tex_sent.contains_key(&(p.clone(), mode)) {
                continue;
            }
            if flip.contains(&p) || self.tex_active.get(&p) != Some(&mode) {
                if !self.v3.set_texture_variant(p.clone(), variant_of(mode)) {
                    return false;
                }
                self.tex_active.insert(p, mode);
            }
        }
        true
    }

    /// A JSON-only change (spec §2.3): simulate again on the stored source materials and send `SetMaterial` for
    /// the shapes whose material changed; a full push when the viewport has not sent a load of this scene yet.
    fn apply_materials(&mut self) {
        let Some(entry) = self.entry.clone() else {
            return;
        };
        let rows = match self.on_screen {
            Some(Shown::Builtin(_)) => vec![self.builtin_scene().1],
            Some(Shown::Nif(id)) => match self.nif.clone().filter(|n| n.load_copy_id == id) {
                Some(nif) => self.nif_rows(&nif, &entry),
                None => {
                    self.want_scene = true;
                    return;
                }
            },
            None => {
                self.want_scene = true;
                return;
            }
        };
        if !slot_norms(&rows.iter().map(|r| &r.material).collect::<Vec<_>>())
            .is_subset(&self.pushed_norms)
        {
            self.want_scene = true;
            return;
        }
        for r in &rows {
            if self.sent.get(&r.id) == Some(&r.material) {
                continue;
            }
            if self.v3.set_material(r.id, r.material.clone()) {
                self.sent.insert(r.id, r.material.clone());
            } else {
                self.want_scene = true;
            }
        }
        self.rows = rows;
    }
}

/// The non-empty slot paths of `materials`.
fn slot_norms(materials: &[&MaterialState]) -> HashSet<String> {
    materials
        .iter()
        .flat_map(|m| m.slots.iter())
        .filter(|t| !t.norm.is_empty())
        .map(|t| t.norm.clone())
        .collect()
}

impl SetPreview {
    /// The app's instance: the saved shaders, the `[meshes]` target, the scan DB path and the D5 cap. No IO.
    pub fn from_settings(s: &crate::settings::GuiSettings) -> SetPreview {
        let cap = match s.render3d.set_preview_max {
            0 => DEFAULT_CAP,
            n => n.clamp(256, 8192),
        };
        SetPreview::new(
            s.render3d.shaders.clone(),
            target_of(&s.meshes),
            crate::model::meshes::default_store_path(),
            cap,
        )
    }
}
