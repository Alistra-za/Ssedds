//! v2 artist checks, Task 10: the load-time budget. Planning with the check stats on must stay within 15 % of
//! planning without them on the local test corpus (DECISIONS Note CL41). Owner data: `#[ignore]`, gated on
//! `SDC_TEXTURES_10K`. Run with `cargo test -p engine --release --test check_budget -- --ignored --nocapture`.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::session::AnalysisCache;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Seconds for one cold plan (a fresh cache, so every texture is analysed).
fn timed(root: &Path, out: &Path, checks: bool) -> f64 {
    let mut cache = AnalysisCache::default();
    cache.thumbnails = true;
    cache.checks = checks;
    let t = Instant::now();
    Engine::new()
        .unwrap()
        .plan_cached(
            BatchRequest {
                recursive: true,
                ..BatchRequest::new(vec![root.to_path_buf()], out.to_path_buf(), Target::Cs)
            },
            &Reporter::new(Arc::new(NullSink), "budget"),
            &CancelToken::default(),
            &cache,
        )
        .unwrap();
    t.elapsed().as_secs_f64()
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

#[test]
#[ignore = "owner data: set SDC_TEXTURES_10K"]
fn plan_time_with_checks_stays_within_15_percent() {
    let Some(root) = std::env::var_os("SDC_TEXTURES_10K").map(PathBuf::from) else {
        eprintln!("SDC_TEXTURES_10K unset: skipped");
        return;
    };
    assert!(root.is_dir(), "{} is not a folder", root.display());
    let out = std::env::temp_dir().join("sdc-check-budget-out");
    let (mut base, mut on) = (vec![], vec![]);
    for _ in 0..3 {
        base.push(timed(&root, &out, false));
        on.push(timed(&root, &out, true));
    }
    let (b, c) = (median(&mut base), median(&mut on));
    eprintln!(
        "base {base:?} -> median {b:.1} s; checks {on:?} -> median {c:.1} s; ratio {:.3}",
        c / b
    );
    assert!(c <= 1.15 * b, "checks {c:.1} s > 1.15 x base {b:.1} s");
}
