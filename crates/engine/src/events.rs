//! Engine events (spec §5.9). The CLI writes them as JSON Lines; the GUI receives the same values.
use crate::codes::Code;
use crate::config::Subtype;
use crate::config::Target;
use crate::policy::ResolvedJob;
use crate::validate::ValidationSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const EVENT_SCHEMA: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Discover,
    Group,
    Load,
    Analyse,
    Resolve,
    Preprocess,
    Mipmaps,
    Compress,
    Write,
    Validate,
}

/// What became of one channel-packing recipe of a set (spec §6.4 "Reporting").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecipeStatus {
    /// Planned as a job.
    Run,
    /// The set already holds this output as a file (`PACK-CONFLICT`); `reason` says which side is used.
    Conflict,
    /// A source could not be read or the sources differ in size.
    Failed,
    /// `packing.enabled = false`: the raw maps stay packing inputs.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeReport {
    /// The packed output, '/'-separated.
    pub output: String,
    pub subtype: Subtype,
    /// Provenance of R, G, B and A (`source:…`, `default:…`, `fill:…`); empty for a mirrored or opaque channel.
    pub channels: Vec<String>,
    pub status: RecipeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Why a file produced no output in this batch (DECISIONS N5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    UpToDate,
    /// `--on-existing skip` kept an existing output.
    OnExistingSkip,
    /// A sub-type or alpha meaning still needs a decision.
    Unresolved,
    /// A blocking code was not acknowledged.
    Blocked,
    /// `IN-DUPLICATE`: two inputs map to one output.
    Duplicate,
    /// `IN-DDS-SKIPPED`.
    DdsInput,
    /// A raw map not packed (`IN-PACKING-INPUT`, `PACK-UNUSED-INPUT`).
    PackingInput,
    /// An RNM normal layer or mask (M6 Track F2): blended into its base normal, never written alone.
    NormalLayer,
    /// `OUT-UNKNOWN-EXISTING`.
    UnknownExisting,
    /// `PACK-CONFLICT`: a pre-packed file replaced under `packing.prefer = raw`.
    PackConflict,
    /// A raw map consumed by a recipe; the event's `job_key` names the packed output.
    PackedSource,
    /// `CHK-LOW-DETAIL`: held by `BatchRequest::low_detail = hold`, not written (v2 artist checks, check 3).
    LowDetail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    BatchStarted {
        target: Target,
        preset: String,
        inputs: usize,
    },
    FileDiscovered {
        output: String,
    },
    SetResolved {
        members: Vec<String>,
        pbr: bool,
        bake: bool,
        /// The set's channel-packing recipes (spec §6.4); absent when it packs nothing.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        recipes: Vec<RecipeReport>,
        /// M6 (DECISIONS Note S): the set's shader and why it was chosen.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shader: Option<crate::shader::ShaderPick>,
        /// SHADER-AUTO / SHADER-UNAVAILABLE, then SHADER-EXTRA-MAP / SHADER-MISSING-MAP for the set.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<crate::codes::Warning>,
    },
    FilePlanned {
        job: Box<ResolvedJob>,
    },
    FileStarted,
    Progress {
        stage: Stage,
        done: u32,
        total: u32,
    },
    Warning(crate::codes::Warning),
    FileDone {
        output: String,
        validation: ValidationSummary,
    },
    FileSkipped {
        /// Serialised as `skip`: `kind` is the event tag.
        #[serde(rename = "skip")]
        kind: SkipKind,
        code: Option<Code>,
        reason: String,
    },
    FileFailed {
        stage: Stage,
        error: String,
    },
    BatchDone {
        converted: u32,
        up_to_date: u32,
        /// Every `file_skipped` that is not up to date and not a packing input.
        skipped: u32,
        /// Of `skipped`, those skipped on purpose (`IN-DDS-SKIPPED`, `PACK-CONFLICT`, `--on-existing skip`); they
        /// do not make the batch exit 1 (DECISIONS Notes I and M9). Absent in older streams.
        #[serde(default)]
        skipped_intentional: u32,
        failed: u32,
        validation_failed: u32,
        cancelled: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineEvent {
    pub schema: u32,
    pub seq: u64,
    /// RFC 3339, UTC, millisecond precision.
    pub ts: String,
    pub batch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The job this event belongs to: its output path relative to the output root (DECISIONS N6). Absent on
    /// batch-level events. Serialised as `job_key`: `file_planned` already carries its `job` object.
    #[serde(default, rename = "job_key", skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(flatten)]
    pub kind: EventKind,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, ev: &EngineEvent);
}

#[derive(Default)]
pub struct VecSink(Mutex<Vec<EngineEvent>>);

impl VecSink {
    pub fn take(&self) -> Vec<EngineEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl EventSink for VecSink {
    fn emit(&self, ev: &EngineEvent) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(ev.clone());
    }
}

pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _: &EngineEvent) {}
}

/// Track A (Note CM): forwards every event with its job key prefixed (`"<Vanilla folder>/"`), so the three folders'
/// rows of one Also export never share a key. Batch-level events (no job) pass unchanged.
pub struct PrefixSink {
    prefix: String,
    inner: Arc<dyn EventSink>,
}

impl PrefixSink {
    pub fn new(prefix: String, inner: Arc<dyn EventSink>) -> Self {
        PrefixSink { prefix, inner }
    }
}

impl EventSink for PrefixSink {
    fn emit(&self, ev: &EngineEvent) {
        let mut ev = ev.clone();
        ev.job = ev.job.map(|k| format!("{}{k}", self.prefix));
        self.inner.emit(&ev);
    }
}

pub struct JsonLinesSink<W: Write + Send>(Mutex<W>);

impl<W: Write + Send> JsonLinesSink<W> {
    pub fn new(w: W) -> Self {
        JsonLinesSink(Mutex::new(w))
    }
}

impl<W: Write + Send> EventSink for JsonLinesSink<W> {
    /// One line per event, written in a single call so a reader never sees half of one. A closed pipe is not
    /// an engine failure, so write errors are dropped.
    fn emit(&self, ev: &EngineEvent) {
        let Ok(mut line) = serde_json::to_vec(ev) else {
            return;
        };
        line.push(b'\n');
        let mut w = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = w.write_all(&line);
        let _ = w.flush();
    }
}

pub struct Reporter {
    sink: Arc<dyn EventSink>,
    seq: AtomicU64,
    /// Held across numbering and delivery so `seq` order equals delivery order even with parallel files.
    order: Mutex<()>,
    pub batch: String,
}

impl Reporter {
    pub fn new(sink: Arc<dyn EventSink>, batch: impl Into<String>) -> Self {
        Reporter {
            sink,
            seq: AtomicU64::new(0),
            order: Mutex::new(()),
            batch: batch.into(),
        }
    }

    pub fn emit(&self, set: Option<&str>, path: Option<&Path>, kind: EventKind) {
        self.emit_job(None, set, path, kind);
    }

    /// Emit an event that belongs to the job whose output key (relative to the output root) is `job`.
    pub fn emit_job(
        &self,
        job: Option<&str>,
        set: Option<&str>,
        path: Option<&Path>,
        kind: EventKind,
    ) {
        let _order = self.order.lock().unwrap_or_else(|e| e.into_inner());
        let ev = EngineEvent {
            schema: EVENT_SCHEMA,
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            ts: now_rfc3339(),
            batch: self.batch.clone(),
            set: set.map(str::to_string),
            path: path.map(|p| p.to_string_lossy().replace('\\', "/")),
            job: job.map(str::to_string),
            kind,
        };
        self.sink.emit(&ev);
    }
}

/// The current time as RFC 3339 UTC with millisecond precision, e.g. `2026-10-09T12:34:56.789Z`.
pub fn now_rfc3339() -> String {
    humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string()
}

/// `<ms since epoch>-<pid>-<n>`: the process-local counter `n` keeps two batches started in the same millisecond
/// (a GUI queue) apart.
pub fn new_batch_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms:x}-{:x}-{n:x}", std::process::id())
}
