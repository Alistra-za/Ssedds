//! The background shader compile (03 §1.4): plain-data jobs in, plain-data results out. Only plain data crosses
//! threads (`ExpandedSource`, `Defines`, `Compiled` are `Send`); the render thread owns every device object.
use crate::compile::{compile, CompiledPair, ProgramKey};
use crate::permutation::Defines;
use crate::source::ExpandedSource;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, PoisonError};

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

fn pair(job: &CompileJob) -> Result<CompiledPair, String> {
    let vs =
        compile(&job.src, &job.vs, "vs_5_0", &job.cache_dir).map_err(|e| format!("vs: {e}"))?;
    let ps =
        compile(&job.src, &job.ps, "ps_5_0", &job.cache_dir).map_err(|e| format!("ps: {e}"))?;
    Ok(CompiledPair { vs, ps })
}

/// Compiles (or reads from the disk cache) both stages. A panic inside the compiler becomes an error result,
/// so a job is never lost and a shape never stays `Pending` for ever.
pub fn run_job(job: CompileJob) -> CompileDone {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pair(&job)))
        .unwrap_or_else(|_| Err(format!("{PANIC_PREFIX} the shader compiler panicked")));
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
