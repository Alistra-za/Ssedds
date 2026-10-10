//! Engine work off the UI thread (M3a spec §3): every call runs on its own thread with a fresh `CancelToken`,
//! catches panics at the thread boundary (spec §7.6) and reports through one channel the UI drains each frame.
use crate::model::items::PlanView;
use crate::model::preview::PreviewOut;
use engine::api::{BatchRequest, BatchSummary, CancelToken};
use engine::events::{EngineEvent, EventKind, EventSink, Reporter, VecSink};
use engine::session::{Session, SourcePreview};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub enum WorkerMsg {
    /// Planning discovered `total` files (the status line reads "Analysing {total} files…").
    PlanProgress {
        generation: u64,
        total: usize,
    },
    Planned {
        generation: u64,
        result: Result<PlanView, String>,
    },
    Event(EngineEvent),
    ExportDone(Result<BatchSummary, String>),
    Source {
        token: u64,
        result: Result<Arc<SourcePreview>, String>,
    },
    Encoded {
        token: u64,
        result: Result<Arc<PreviewOut>, String>,
    },
    Panicked {
        what: &'static str,
        message: String,
    },
}

/// Forwards every engine event into the channel and nothing else, so it never slows the encoders (plan M3 carry).
pub struct ChannelSink(pub Sender<WorkerMsg>);

impl EventSink for ChannelSink {
    fn emit(&self, ev: &EngineEvent) {
        let _ = self.0.send(WorkerMsg::Event(ev.clone()));
    }
}

/// Collects a plan's events for `PlanView::build` and reports the file count once `batch_started` arrives.
struct PlanSink {
    events: Arc<VecSink>,
    tx: Sender<WorkerMsg>,
    generation: u64,
}

impl EventSink for PlanSink {
    fn emit(&self, ev: &EngineEvent) {
        if let EventKind::BatchStarted { inputs, .. } = &ev.kind {
            let _ = self.tx.send(WorkerMsg::PlanProgress {
                generation: self.generation,
                total: *inputs,
            });
        }
        self.events.emit(ev);
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
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
        .name(format!("sdc-gui-{what}"))
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

pub fn spawn_plan(
    tx: Sender<WorkerMsg>,
    session: Arc<Session>,
    req: BatchRequest,
    cancel: CancelToken,
    generation: u64,
    root: PathBuf,
) {
    spawn_guarded(tx, "plan", move |tx| {
        let events = Arc::new(VecSink::default());
        let sink = Arc::new(PlanSink {
            events: events.clone(),
            tx: tx.clone(),
            generation,
        });
        let rep = Reporter::new(sink, engine::events::new_batch_id());
        let result = session
            .plan(req, &rep, &cancel)
            .map(|plan| PlanView::build(&plan, &events.take(), &session, &root))
            .map_err(|e| e.to_string());
        let _ = tx.send(WorkerMsg::Planned { generation, result });
    });
}
