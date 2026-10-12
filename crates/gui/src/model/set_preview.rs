//! The live set preview (P1-A, spec §2.3, 02): the Textures view's own 3D panel, showing the selected set on a game
//! mesh patched in memory as PGPatcher 1.2.0 would patch it (`pgpsim`), with this session's encodes of the set's
//! textures. No egui types; `ui::set_preview` draws it.
//!
//! Part 1: per-member preview encodes → DDS bytes served as `FileSource::Memory`, fresh/stale per member, paused
//! during an export. Results come back on this module's own channel, so `worker.rs` needs no new message.
use crate::model::items::{ItemKind, ItemStatus, TextureItem};
use crate::model::preview::DEBOUNCE;
use crate::model::sets::{SetView, SlotFill, SlotKind};
use crate::model::viewport3d::FileSource;
use crate::model::AppModel;
use crate::pgp::prefix::output_rel;
use crate::pgp::writer::EntryInput;
use engine::api::CancelToken;
use engine::dds::{dds_bytes, resolve_header, DdsDesc};
use engine::manifest::sha256_json;
use engine::policy::ResolvedJob;
use engine::session::{pack_key, Session};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

/// Memory ids are process-wide: `FileSource::Memory` compares by id, across encoders.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// D5: the preview's encode size cap (`[render3d] set_preview_max` in `gui.toml`, 0 = this).
pub const DEFAULT_CAP: u32 = 2048;
pub const WAITS_FOR_EXPORT: &str = "Preview encoding waits for the export to finish";
const BLOCKED_ERROR: &str = "the texture has an error";
const BLOCKED_NOTHING: &str = "nothing to encode";

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
                None => format!("{p}:{:?}:{:?}", job.input, job.source),
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
    cancel: Option<CancelToken>,
}

struct EncDone {
    kind: SlotKind,
    token: u64,
    cap: u32,
    result: Result<(Vec<u8>, f64), String>,
}

/// The selected set's encodes, one per filled slot.
pub struct Encoder {
    pub members: BTreeMap<SlotKind, Member>,
    pub cap: u32,
    /// An export is running: nothing starts (02 §2.3).
    pub paused: bool,
    pub error: Option<String>,
    next_token: u64,
    hold: bool,
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
        .name("sdc-gui-set-preview".into())
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

/// One member's bytes: the preview encode within `cap`, made into a DDS exactly as `Session::preview` builds it
/// (`session.rs:283-290`), or a DDS input's own file.
pub fn encode(
    session: &Session,
    source: &Source,
    cap: u32,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    match source {
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
            paused: false,
            error: None,
            next_token: 0,
            hold: false,
            tx,
            rx,
        }
    }

    /// A new size cap: every member that has something to encode goes stale (old bytes on show), in-flight work
    /// is cancelled and its late result dropped.
    pub fn set_cap(&mut self, cap: u32, now: Instant) {
        self.cap = cap;
        for mem in self.members.values_mut() {
            if let Some(c) = mem.cancel.take() {
                c.cancel();
            }
            mem.token = 0;
            if !matches!(mem.state, EncState::Blocked(_)) {
                mem.state = EncState::Stale(mem.state.shown().cloned());
                mem.cap = cap;
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
        let _ = self.tx.send(EncDone {
            kind,
            token,
            cap,
            result,
        });
    }

    /// Another set: cancel and forget every member.
    pub fn reset(&mut self) {
        for m in self.members.values() {
            if let Some(c) = &m.cancel {
                c.cancel();
            }
        }
        self.members.clear();
        self.error = None;
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        for m in self.members.values() {
            if let Some(c) = &m.cancel {
                c.cancel();
            }
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
                if let Some(c) = &mem.cancel {
                    c.cancel();
                }
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
                Some(mem) if same_bytes(&mem.slot, &slot) && mem.cap == self.cap => mem.slot = slot,
                Some(mem) => {
                    if let Some(c) = mem.cancel.take() {
                        c.cancel();
                    }
                    mem.token = 0;
                    mem.cap = self.cap;
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
                            cancel: None,
                        },
                    );
                }
            }
        }
    }

    /// Starts every due encode, one thread each (a set has at most 9 members). Nothing starts while an export
    /// runs or without a session.
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
            let cancel = CancelToken::default();
            mem.cancel = Some(cancel.clone());
            mem.state = EncState::Encoding(mem.state.shown().cloned());
            if self.hold {
                continue;
            }
            let (session, source, cap, kind) =
                (session.clone(), mem.source.clone(), self.cap, *kind);
            spawn(
                self.tx.clone(),
                move || {
                    let t0 = Instant::now();
                    let result = encode(&session, &source, cap, &cancel).map(|b| (b, ms(t0)));
                    EncDone {
                        kind,
                        token,
                        cap,
                        result,
                    }
                },
                move |e| EncDone {
                    kind,
                    token,
                    cap,
                    result: Err(format!("internal error in a preview encode: {e}")),
                },
            );
        }
    }

    /// Drains finished encodes. True when a member's bytes changed or it failed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(EncDone {
            kind,
            token,
            cap,
            result,
        }) = self.rx.try_recv()
        {
            let Some(mem) = self.members.get_mut(&kind) else {
                continue;
            };
            // Superseded: a newer edit, a block, a cap change (all zero or replace the token).
            if mem.token == 0 || mem.token != token {
                continue;
            }
            // The live encode ran at another cap than the current one: useless, schedule it again.
            if cap != self.cap {
                if let Some(c) = mem.cancel.take() {
                    c.cancel();
                }
                mem.token = 0;
                mem.cap = self.cap;
                mem.state = EncState::Stale(mem.state.shown().cloned());
                mem.due = Some(Instant::now());
                changed = true;
                continue;
            }
            mem.cancel = None;
            mem.token = 0;
            mem.state = match result {
                Ok((dds, ms)) => EncState::Fresh(Arc::new(Encoded {
                    id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                    dds: Arc::new(dds),
                    ms,
                })),
                Err(e) => EncState::Failed(e),
            };
            changed = true;
        }
        changed
    }

    pub fn settled(&self) -> bool {
        self.members.values().all(|m| m.state.settled())
    }

    /// Work is waiting or running (false while paused: the export keeps the app repainting).
    pub fn busy(&self) -> bool {
        !self.paused && !self.settled()
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
