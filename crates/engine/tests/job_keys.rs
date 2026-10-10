//! M3a Task 5: every per-file event carries its job key; skips carry a kind; pack sources get a terminal event.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture(dir: &Path) {
    let r = dir.join("in");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(r.join("iron.png"))
        .unwrap();
    for (name, v) in [("iron_roughness.png", 120u8), ("iron_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(name))
            .unwrap();
    }
}

fn convert(dir: &Path) -> Vec<EngineEvent> {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.overrides = vec![parse_set("file:iron.png.subtype=diffuse").unwrap()];
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    sink.take()
}

fn kind(e: &EngineEvent) -> String {
    serde_json::to_value(&e.kind).unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn every_per_file_event_carries_its_job_key() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = convert(d.path());
    let per_file = [
        "file_discovered",
        "file_planned",
        "file_started",
        "progress",
        "file_done",
        "file_skipped",
        "file_failed",
    ];
    for e in ev.iter().filter(|e| per_file.contains(&kind(e).as_str())) {
        assert!(
            e.job.is_some(),
            "{} without a job key: {:?}",
            kind(e),
            e.path
        );
    }
    let done: Vec<&str> = ev
        .iter()
        .filter(|e| kind(e) == "file_done")
        .map(|e| e.job.as_deref().unwrap())
        .collect();
    assert!(done.contains(&"iron.dds"));
    assert!(done.contains(&"iron_rmaos.dds"));
}

#[test]
fn skips_carry_their_kind_and_pack_sources_get_a_terminal_event() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = convert(d.path());
    let packed: Vec<(&str, &str)> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileSkipped {
                kind: SkipKind::PackedSource,
                ..
            } => Some((e.path.as_deref().unwrap(), e.job.as_deref().unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(packed.len(), 2, "{packed:?}");
    assert!(packed.iter().all(|(_, job)| *job == "iron_rmaos.dds"));
    assert!(packed
        .iter()
        .any(|(p, _)| p.ends_with("iron_roughness.png")));
    assert!(packed.iter().any(|(p, _)| p.ends_with("iron_ao.png")));
    // A second run: both outputs are up to date; a copied DDS input is a dds_input skip.
    std::fs::copy(d.path().join("out/iron.dds"), d.path().join("in/extra.dds")).unwrap();
    let ev = convert(d.path());
    let kinds: Vec<SkipKind> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FileSkipped { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == SkipKind::UpToDate).count(),
        2
    );
    assert!(kinds.contains(&SkipKind::DdsInput));
}

#[test]
fn file_planned_keeps_its_job_object_beside_the_job_key() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let ev = convert(d.path());
    let planned = ev.iter().find(|e| kind(e) == "file_planned").unwrap();
    let v = serde_json::to_value(planned).unwrap();
    assert!(v["job"].is_object(), "{v}");
    assert!(v["job_key"].is_string(), "{v}");
    assert_eq!(serde_json::from_value::<EngineEvent>(v).unwrap(), *planned);
}
