//! Task 9: the render3d side of the timing log, driven through the no-pool seam (`take_jobs` + `run_job`) on
//! this thread, so `perf_log::capture` sees every event.
mod common;
use common::{fixture, renderer, FLAT};
use render3d::cache::{prune_logged, PruneLimits};
use render3d::{parse, perf_log, run_job};
use serde_json::Value;
use std::time::{Duration, SystemTime};

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

fn named<'a>(ev: &'a [Value], n: &str) -> Vec<&'a Value> {
    ev.iter().filter(|e| e["event"] == n).collect()
}

fn first_async_load(cache: &std::path::Path) {
    let mut r = renderer(cache, FLAT);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    for j in r.take_jobs() {
        r.apply_compiled(run_job(j));
    }
}

#[test]
fn a_cold_async_load_logs_pending_compile_misses_and_ready() {
    let cache = tempfile::tempdir().unwrap();
    let ((), ev) = perf_log::capture(|| first_async_load(cache.path()));
    let pending = named(&ev, "shape_pending");
    let ready = named(&ev, "shape_ready");
    assert_eq!((pending.len(), ready.len()), (2, 2), "{ev:?}");
    for e in &ready {
        assert_eq!(e["ok"], true);
        assert!(e["ms_pending"].as_f64().unwrap() >= 0.0);
    }
    assert_eq!(pending[0]["showing_previous"], false);
    let compiles = named(&ev, "compile");
    assert!(!compiles.is_empty());
    for c in compiles {
        assert_eq!(
            (c["cache"].as_str(), c["ok"].as_bool()),
            (Some("miss"), Some(true)),
            "{c}"
        );
        assert_eq!(c["key"].as_str().unwrap().len(), 12);
        assert!(c["ms"].as_f64().unwrap() > 0.0);
        assert!(c.get("error").is_none());
    }
    // pending comes before ready, by shape
    let pos = |n: &str| ev.iter().position(|e| e["event"] == n).unwrap();
    assert!(pos("shape_pending") < pos("shape_ready"));
}

#[test]
fn a_warm_cache_logs_compile_hits_and_no_pending_shape() {
    let cache = tempfile::tempdir().unwrap();
    first_async_load(cache.path()); // fills the disk cache
    let ((), ev) = perf_log::capture(|| first_async_load(cache.path()));
    let compiles = named(&ev, "compile");
    assert!(!compiles.is_empty(), "{ev:?}");
    assert!(
        compiles
            .iter()
            .all(|c| c["cache"] == "hit" && c["ok"] == true),
        "{ev:?}"
    );
    assert!(
        named(&ev, "shape_pending").is_empty(),
        "a disk hit is applied at once"
    );
}

#[test]
fn a_failing_program_logs_ok_false_with_the_first_error_line() {
    let cache = tempfile::tempdir().unwrap();
    let ((), ev) = perf_log::capture(|| {
        let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
        r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &none); // one shape's ps does not compile (Task 5)
    });
    let bad: Vec<_> = named(&ev, "compile")
        .into_iter()
        .filter(|c| c["ok"] == false)
        .collect();
    assert_eq!(bad.len(), 1, "{ev:?}");
    let e = bad[0]["error"].as_str().unwrap();
    assert!(e.starts_with("ps:") && !e.contains('\n'), "{e}");
    assert_eq!(
        named(&ev, "shape_ready")
            .iter()
            .filter(|e| e["ok"] == false)
            .count(),
        1
    );
}

#[test]
fn a_prune_logs_what_it_removed() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".tmp-stale"), [7u8; 5]).unwrap();
    std::fs::write(d.path().join("keep.json"), [1u8; 3]).unwrap();
    let later = SystemTime::now() + Duration::from_secs(3 * 86_400); // the files are 3 days old at `later`
    let (rep, ev) = perf_log::capture(|| prune_logged(d.path(), &PruneLimits::default(), later));
    let p = named(&ev, "prune");
    assert_eq!(p.len(), 1, "{ev:?}");
    assert_eq!(p[0]["removed"], rep.removed);
    assert_eq!(p[0]["bytes_before"], 8);
    assert_eq!(p[0]["bytes_after"], rep.bytes_after);
    assert!(p[0]["ms"].as_f64().unwrap() >= 0.0);
    assert!(rep.removed >= 1);
}
