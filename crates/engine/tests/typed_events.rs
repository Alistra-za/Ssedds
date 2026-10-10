//! M3a Task 4: `file_planned.job` is a typed `ResolvedJob` (event schema 2) and unresolved jobs name their needs.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, Need, ResolvedJob};
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    // Varying alpha: a diffuse with a mesh-dependent alpha meaning.
    ImageBuffer::from_fn(64, 64, |x, _| {
        Rgba([200u8, 30, 30, if x < 32 { 255 } else { 0 }])
    })
    .save(r.join("leaf.png"))
    .unwrap();
    // `_normal` is a vocabulary term with no stated green convention.
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("foo_normal.png"))
        .unwrap();
}

fn events(dir: &Path, overrides: &[&str]) -> Vec<EngineEvent> {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.overrides = overrides.iter().map(|s| parse_set(s).unwrap()).collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    sink.take()
}

fn planned(ev: &[EngineEvent], name: &str) -> ResolvedJob {
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::FilePlanned { job } if job.input.ends_with(name) => Some((**job).clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_planned for {name}"))
}

fn needs(j: &ResolvedJob) -> Vec<Need> {
    match &j.status {
        JobStatus::Unresolved { needs, .. } => needs.clone(),
        _ => vec![],
    }
}

#[test]
fn file_planned_round_trips_as_a_typed_job() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = events(d.path(), &[]);
    let mut seen = 0;
    for e in ev
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FilePlanned { .. }))
    {
        assert_eq!(e.schema, 2);
        let text = serde_json::to_string(e).unwrap();
        let back: EngineEvent = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, e);
        seen += 1;
    }
    assert_eq!(seen, 3);
}

#[test]
fn unresolved_jobs_name_what_they_need() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = events(d.path(), &[]);
    assert!(needs(&planned(&ev, "leaf.png")).contains(&Need::Subtype));
    assert!(needs(&planned(&ev, "foo_normal.png")).contains(&Need::GreenConvention));
    let ev = events(d.path(), &["file:leaf.png.subtype=diffuse"]);
    assert_eq!(needs(&planned(&ev, "leaf.png")), vec![Need::AlphaMeaning]);
}
