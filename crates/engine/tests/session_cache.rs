//! v2 artist checks, Task 1: the session's analysis cache carries the check stats, and the CLI's computes none.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::policy::ResolvedJob;
use engine::session::AnalysisCache;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

/// A bumpy tangent-space normal, encoded to 8-bit RGB.
fn bumpy(path: &Path) {
    ImageBuffer::from_fn(64, 64, |x, y| {
        let (u, v) = (x as f32 / 64.0, y as f32 / 64.0);
        let (nx, ny) = (
            0.4 * (u * 12.0).sin() * (v * 7.0).cos(),
            0.4 * (v * 9.0).sin(),
        );
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    })
    .save(path)
    .unwrap();
}

fn fixture(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    bumpy(&dir.join("a_n.png"));
    bumpy(&dir.join("wall_normal.png"));
    ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 4) as u8]))
        .save(dir.join("a_p.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(dir.join("a.png"))
        .unwrap();
}

fn plan(dir: &Path, cache: &AnalysisCache) -> Plan {
    Engine::new()
        .unwrap()
        .plan_cached(
            BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs),
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
            cache,
        )
        .unwrap()
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

const NAMES: [&str; 4] = ["a_n.png", "wall_normal.png", "a_p.png", "a.png"];

#[test]
fn a_session_cache_serves_check_stats_and_the_cli_cache_computes_none() {
    let d = tempfile::tempdir().unwrap();
    fixture(&d.path().join("in"));
    let mut c = AnalysisCache::default();
    c.thumbnails = true;
    c.checks = true;
    let p = plan(d.path(), &c);
    let stats = |p: &Plan, name: &str| c.analysis(job(p, name)).unwrap().checks;
    let first: Vec<_> = NAMES.iter().map(|n| stats(&p, n).unwrap()).collect();
    for n in ["a_n.png", "wall_normal.png"] {
        let s = stats(&p, n).unwrap();
        assert!(s.curl.is_some() && s.steep.is_none(), "{n}: {s:?}");
    }
    assert!(stats(&p, "a_p.png").unwrap().steep.is_some());
    assert!(stats(&p, "a.png").unwrap().curl.is_none());
    assert_eq!(first[0].texels, 64 * 64);

    // A re-plan loads nothing (Note AV4) and serves the same stats from the cache.
    let loads = c.loads();
    let p2 = plan(d.path(), &c);
    assert_eq!(c.loads(), loads, "a warm re-plan loads nothing");
    let second: Vec<_> = NAMES.iter().map(|n| stats(&p2, n).unwrap()).collect();
    assert_eq!(first, second);

    // The CLI's throwaway cache computes no stats.
    let cli = AnalysisCache::default();
    let p = plan(d.path(), &cli);
    for j in &p.jobs {
        assert_eq!(
            cli.analysis(j).map(|a| a.checks),
            Some(None),
            "{:?}",
            j.input
        );
    }
}
