//! Engine work off the UI thread (M3a spec §3): every call runs on its own thread with a fresh `CancelToken`,
//! catches panics at the thread boundary (spec §7.6) and reports through one channel the UI drains each frame.
use crate::model::items::PlanView;
use crate::model::preview::{dds_preview, decode_levels, PreviewOut};
use encoders_sys::Bc1Model;
use engine::api::{BatchRequest, BatchSummary, CancelToken, EngineError, PlanProgress};
use engine::events::{EngineEvent, EventSink, Reporter, VecSink};
use engine::policy::ResolvedJob;
use engine::session::{Session, SourcePreview};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Why a plan failed; `config` is true for an invalid preset or config file (`EngineError::Config`), which the
/// model answers by ignoring `sdc.toml` (M3a spec §7.4).
#[derive(Debug, Clone)]
pub struct PlanFailure {
    pub message: String,
    pub config: bool,
}

/// What one scan-database read returns (`AppModel::refresh_mesh_votes`).
#[derive(Default)]
pub struct MeshEvidence {
    pub votes: std::collections::BTreeMap<String, Vec<engine::shader::Shader>>,
    pub alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
    pub gras: std::collections::BTreeSet<String>,
    /// PBR grass design §4.2: `meshes::gras_all_diffuses`.
    pub gras_all: std::collections::BTreeSet<String>,
    pub tree_anim: std::collections::BTreeMap<String, bool>,
}

pub enum WorkerMsg {
    /// Planning has decoded `done` of `total` discovered files (DECISIONS AG91; the header shows a bar).
    PlanProgress {
        generation: u64,
        done: usize,
        total: usize,
    },
    Planned {
        generation: u64,
        result: Result<PlanView, PlanFailure>,
    },
    /// A scan-database read for the plan's sets ended (stale generations are dropped).
    MeshVotes {
        generation: u64,
        result: Result<MeshEvidence, String>,
    },
    /// Item 9: the "Used by meshes" read of generation `generation` ended (a stale one is dropped).
    UsedBy {
        generation: u64,
        result: crate::model::used_by::UsedBy,
    },
    /// Note AN: a locked slot's original textures were read (a stale generation is dropped).
    OriginalTextures {
        generation: u64,
        key: (String, u8),
        result: Result<Vec<String>, String>,
    },
    /// The scan's texture and mesh paths were read (`AppModel::refresh_known_paths`).
    KnownPaths(Result<crate::model::known_paths::KnownPaths, String>),
    Event(EngineEvent),
    ExportDone(Result<BatchSummary, String>),
    Source {
        token: u64,
        result: Result<Arc<SourcePreview>, String>,
    },
    /// The error heatmap of job `id` (`AppModel::heatmap_frame`); `None` when that mip has no source side.
    Heatmap {
        id: u64,
        image: Option<Arc<crate::model::heatmap::HeatImage>>,
    },
    /// The RNM blended thumbnail of job `id` (`AppModel::rnm_preview_frame`); `None` when the blend failed.
    RnmPreview {
        id: u64,
        image: Option<Arc<crate::model::rnm_preview::RnmThumb>>,
    },
    /// Source sizes read from file headers (`AppModel::kick_dims`).
    Dims(Vec<(crate::model::out_size::DimKey, Option<(u32, u32)>)>),
    Encoded {
        token: u64,
        result: Result<Arc<PreviewOut>, String>,
    },
    Panicked {
        what: &'static str,
        message: String,
    },
    /// Meshes view (plan A Task 12): a scan phase advanced.
    ScanProgress {
        done: usize,
        total: usize,
        phase: &'static str,
    },
    /// Meshes view: the scan ended. The error keeps its kind (`PgpRunning`, `Cancelled`, a database error).
    Scanned(Result<modscan::scan::Report, modscan::ScanError>),
    /// Meshes view 3D panel (plan B Task B8): a reply of the render thread or the CS locate worker.
    Viewport3d(crate::model::viewport3d::V3Reply),
    /// The NVTT probe of generation `generation` ended (`AppModel::refresh_nvtt`; a stale one is dropped).
    Nvtt {
        generation: u64,
        probe: engine::nvtt::NvttProbe,
    },
    /// A Save report write ended (AG130): the path and the error, if any.
    ReportSaved {
        path: std::path::PathBuf,
        result: Result<(), String>,
    },
}

/// Forwards every engine event into the channel and nothing else, so it never slows the encoders (plan M3 carry).
pub struct ChannelSink(pub Sender<WorkerMsg>);

impl EventSink for ChannelSink {
    fn emit(&self, ev: &EngineEvent) {
        let _ = self.0.send(WorkerMsg::Event(ev.clone()));
    }
}

/// How often the plan ticker reports.
const PLAN_TICK: Duration = Duration::from_millis(100);

/// AG91: the planner counts each planned file (cache hits included) in the request's `PlanProgress`, and nothing
/// else; this ticker reports those counters until `stop` is dropped. Each plan has its own counters, so a cancelled
/// plan still draining never moves the next plan's bar.
fn plan_ticker(
    tx: Sender<WorkerMsg>,
    progress: PlanProgress,
    generation: u64,
    stop: Receiver<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut last = None;
        while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(PLAN_TICK) {
            let total = progress.total.load(Ordering::Relaxed);
            if total == 0 {
                continue;
            }
            let done = progress.done.load(Ordering::Relaxed);
            if last != Some(done) {
                last = Some(done);
                let _ = tx.send(WorkerMsg::PlanProgress {
                    generation,
                    done,
                    total,
                });
            }
        }
    })
}

pub(crate) fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Run `f` on a new thread; a panic becomes `WorkerMsg::Panicked` and the window keeps running.
pub fn spawn_guarded(
    tx: Sender<WorkerMsg>,
    what: &'static str,
    f: impl FnOnce(&Sender<WorkerMsg>) + Send + 'static,
) {
    std::thread::Builder::new()
        .name(format!("tessalith-{what}"))
        .spawn(move || {
            if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&tx))) {
                let _ = tx.send(WorkerMsg::Panicked {
                    what,
                    message: panic_text(&*p),
                });
            }
        })
        .expect("spawn a worker thread");
}

/// Plan (through the session's cache) and run, streaming every event into the channel, then send `ExportDone`.
pub fn spawn_export(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    req: BatchRequest,
    cancel: CancelToken,
) {
    spawn_guarded(tx, "export", move |tx| {
        let rep = Reporter::new(
            Arc::new(ChannelSink(tx.clone())),
            engine::events::new_batch_id(),
        );
        let result = session
            .plan(req, &rep, &cancel)
            .and_then(|plan| session.run(plan, &rep, &cancel))
            .map_err(|e| e.to_string());
        let _ = tx.send(WorkerMsg::ExportDone(result));
    });
}

/// A plan, with a panic turned into this plan's failure, so a superseded plan's panic is dropped by its
/// generation like any stale result (audit left-out).
fn plan_result(f: impl FnOnce() -> Result<PlanView, PlanFailure>) -> Result<PlanView, PlanFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        Err(PlanFailure {
            config: false,
            message: format!("internal error in plan: {}", panic_text(&*p)),
        })
    })
}

pub fn spawn_plan(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    req: BatchRequest,
    cancel: CancelToken,
    generation: u64,
    root: PathBuf,
) {
    spawn_guarded(tx, "plan", move |tx| {
        let mut req = req;
        let progress = PlanProgress::default();
        req.plan_progress = Some(progress.clone());
        let events = Arc::new(VecSink::default());
        let (stop, stop_rx) = std::sync::mpsc::channel();
        let ticker = plan_ticker(tx.clone(), progress, generation, stop_rx);
        let rep = Reporter::new(events.clone(), engine::events::new_batch_id());
        let result = plan_result(|| {
            session
                .plan(req, &rep, &cancel)
                .map(|plan| PlanView::build(&plan, &events.take(), &session, &root))
                .map_err(|e| PlanFailure {
                    config: matches!(e, EngineError::Config(_)),
                    message: e.to_string(),
                })
        });
        // The ticker's last report is queued before `Planned`, so none of this generation follows it.
        drop(stop);
        let _ = ticker.join();
        let _ = tx.send(WorkerMsg::Planned { generation, result });
    });
}

/// The source side of the preview: the chain the encoder would receive (nothing is encoded).
pub fn spawn_source(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    job: ResolvedJob,
    max: u32,
    cancel: CancelToken,
    token: u64,
) {
    spawn_guarded(tx, "preview", move |tx| {
        let result = session
            .preview_source_stored(&job, max, &cancel)
            .map(Arc::new)
            .map_err(|e| format!("{e:?}"));
        let _ = tx.send(WorkerMsg::Source { token, result });
    });
}

/// Both sides: encode every mip within `max` and decode them again for the heatmap and the fallback path.
pub fn spawn_encode(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    job: ResolvedJob,
    max: u32,
    cancel: CancelToken,
    token: u64,
) {
    spawn_guarded(tx, "preview", move |tx| {
        let bc1 = session.bc1_model();
        let result = session
            .preview(&job, max, &cancel)
            .map(|r| {
                Arc::new(PreviewOut {
                    decoded: decode_levels(&r, bc1),
                    result: r,
                })
            })
            .map_err(|e| format!("{e:?}"));
        let _ = tx.send(WorkerMsg::Encoded { token, result });
    });
}

/// A DDS input shows only its own bytes: read the file, keep the mips within `max`, decode them.
pub fn spawn_dds(tx: Sender<WorkerMsg>, path: PathBuf, max: u32, bc1: Bc1Model, token: u64) {
    spawn_guarded(tx, "preview", move |tx| {
        let result = dds_preview(&path, max, bc1).map(Arc::new);
        let _ = tx.send(WorkerMsg::Encoded { token, result });
    });
}

/// Meshes view (plan A Task 12): scan `source` into the store at `db` (opened on this thread; the UI keeps its own
/// read connection). `procs` lists running process names for the PGPatcher gate; `skip_derived` scans without the
/// PGPatcher output. Progress and the result go to `tx`; a panic becomes `Panicked { what: "scan" }`.
pub fn start_scan(
    source: modscan::scan::Source,
    db: PathBuf,
    skip_derived: bool,
    procs: fn() -> Vec<String>,
    tx: Sender<WorkerMsg>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    spawn_guarded(tx, "scan", move |tx| {
        let progress = |p: modscan::scan::Progress| {
            let _ = tx.send(WorkerMsg::ScanProgress {
                done: p.done,
                total: p.total,
                phase: p.phase,
            });
        };
        let result = modscan::store::Store::open(&db).and_then(|mut store| {
            modscan::scan::run_with(
                &source,
                &mut store,
                &cancel,
                &progress,
                &procs,
                skip_derived,
            )
        });
        let _ = tx.send(WorkerMsg::Scanned(result));
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panicking_plan_is_a_plan_failure_of_its_generation() {
        let r = super::plan_result(|| panic!("boom"));
        let Err(f) = r else { panic!("a failure") };
        assert!(f.message.contains("boom") && !f.config, "{}", f.message);
    }
}
