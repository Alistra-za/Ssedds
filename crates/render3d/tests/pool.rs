//! The compile pool: real worker threads, below-normal priority, results through the sink.
mod common;
use common::{fixture, renderer, FLAT};
use render3d::{parse, CompilePool, ShapeStatus};
use std::time::Duration;
use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority};

#[test]
fn the_pool_compiles_off_thread_at_below_normal_priority() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(
        &parse(&fixture("TestNifFile_Furniture_Col_SE.nif")),
        &|_| None,
    );
    let jobs = r.take_jobs();
    let n = jobs.len();
    assert!(n >= 1);
    let (tx, rx) = std::sync::mpsc::channel();
    // The sink runs on the worker thread: read that thread's priority there.
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
        // SAFETY: the pseudo-handle of the calling thread is always valid.
        let prio = unsafe { GetThreadPriority(GetCurrentThread()) };
        let _ = tx.send((d, prio, std::thread::current().id()));
    });
    for j in jobs {
        pool.submit(j);
    }
    for _ in 0..n {
        let (done, prio, tid) = rx.recv_timeout(Duration::from_secs(60)).expect("a result");
        assert_eq!(prio, -1, "THREAD_PRIORITY_BELOW_NORMAL");
        assert_ne!(tid, std::thread::current().id());
        assert!(done.result.is_ok(), "{:?}", done.result);
        assert!(r.apply_compiled(done));
    }
    assert!(r.shapes().iter().all(|s| s.status == ShapeStatus::Drawing));
}

#[test]
fn a_failing_job_is_delivered_as_an_error() {
    let cache = tempfile::tempdir().unwrap();
    let set = render3d::source::SourceSet::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures").into(),
    );
    // `tiny.hlsl` is a pixel shader (`SV_Position` in, `SV_Target` out): as vs_5_0 it does not compile.
    let src = set.get(std::path::Path::new("tiny.hlsl")).unwrap();
    let job = render3d::CompileJob {
        key: render3d::compile::ProgramKey("k".into()),
        src,
        vs: render3d::permutation::Defines::default(),
        ps: render3d::permutation::Defines::default(),
        cache_dir: cache.path().to_path_buf(),
    };
    let done = render3d::run_job(job);
    assert_eq!(done.key.0, "k");
    assert!(
        done.result.as_ref().is_err_and(|e| e.starts_with("vs:")),
        "{:?}",
        done.result
    );
}
