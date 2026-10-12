//! The background shader compile (03 §1.4): plain-data jobs in, plain-data results out. Only plain data crosses
//! threads (`ExpandedSource`, `Defines`, `Compiled` are `Send`); the render thread owns every device object.
use crate::compile::{cached, compile, Compiled, CompiledPair, ProgramKey};
use crate::perf_log;
use crate::permutation::Defines;
use crate::source::ExpandedSource;
use serde_json::json;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// Spec §2.2: two workers, below-normal priority, not a setting (the encoder shares the CPU).
pub const WORKERS: usize = 2;

/// Starts the error text of a job whose compiler panicked (not memoised by the renderer).
pub const PANIC_PREFIX: &str = "internal error:";

pub struct CompileJob {
    /// `ProgramKey::pair` of the two stage keys.
    pub key: ProgramKey,
    pub src: Arc<ExpandedSource>,
    pub vs: Defines,
    pub ps: Defines,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CompileDone {
    pub key: ProgramKey,
    pub result: Result<Arc<CompiledPair>, String>,
}

/// One stage: a disk-cache hit, or a compile. The flag says which.
fn stage(job: &CompileJob, d: &Defines, target: &str) -> (Result<Compiled, String>, bool) {
    match cached(&job.src, d, target, &job.cache_dir) {
        Some(c) => (Ok(c), true),
        None => (compile(&job.src, d, target, &job.cache_dir), false),
    }
}

/// Both stages; the flag is true only when both came from the disk cache.
fn pair(job: &CompileJob) -> (Result<CompiledPair, String>, bool) {
    let (vs, vs_hit) = stage(job, &job.vs, "vs_5_0");
    let vs = match vs {
        Ok(v) => v,
        Err(e) => return (Err(format!("vs: {e}")), false),
    };
    let (ps, ps_hit) = stage(job, &job.ps, "ps_5_0");
    match ps {
        Ok(ps) => (Ok(CompiledPair { vs, ps }), vs_hit && ps_hit),
        Err(e) => (Err(format!("ps: {e}")), false),
    }
}

/// Compiles (or reads from the disk cache) both stages and logs one `compile` line. A panic inside the compiler
/// becomes an error result, so a job is never lost and a shape never stays `Pending` for ever.
pub fn run_job(job: CompileJob) -> CompileDone {
    let t0 = Instant::now();
    let (result, hit) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pair(&job)))
        .unwrap_or_else(|_| {
            (
                Err(format!("{PANIC_PREFIX} the shader compiler panicked")),
                false,
            )
        });
    let mut f = json!({
        "key": perf_log::short(&job.key.0),
        "ms": perf_log::ms_since(t0),
        "cache": if hit { "hit" } else { "miss" },
        "ok": result.is_ok(),
    });
    if let Err(e) = &result {
        f["error"] = json!(perf_log::first_line(e));
    }
    perf_log::emit("compile", f);
    CompileDone {
        key: job.key,
        result: result.map(Arc::new),
    }
}

fn lower_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: the pseudo-handle of the calling thread is always valid.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

/// `threads` workers at below-normal priority. Every finished job goes to `sink` on the worker thread (the GUI's
/// sink sends it into the render loop's own queue). Dropping the pool lets idle workers exit; a job already
/// running finishes first.
pub struct CompilePool {
    tx: Sender<CompileJob>,
}

impl CompilePool {
    pub fn new(threads: usize, sink: impl Fn(CompileDone) + Send + Sync + 'static) -> CompilePool {
        let (tx, rx) = channel::<CompileJob>();
        let rx = Arc::new(Mutex::new(rx));
        let sink = Arc::new(sink);
        for n in 0..threads.max(1) {
            let (rx, sink) = (rx.clone(), sink.clone());
            let _ = std::thread::Builder::new()
                .name(format!("shader-compile-{n}"))
                .spawn(move || {
                    lower_priority();
                    loop {
                        // The lock is held while waiting: the other workers queue behind it, which is fine.
                        let job = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        let Ok(job) = job else { break };
                        sink(run_job(job));
                    }
                });
        }
        CompilePool { tx }
    }

    pub fn submit(&self, job: CompileJob) {
        let _ = self.tx.send(job);
    }
}
