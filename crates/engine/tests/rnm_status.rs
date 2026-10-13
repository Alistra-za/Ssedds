//! M6 Track F2 Task 3: layer files are hidden jobs, never written alone (F2 spec §2.2, Review Focus 2).
use engine::api::*;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::events::*;
use engine::policy::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn png(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 255u8]))
        .save(p)
        .unwrap();
}

pub fn plan_dir(dir: &Path, sets: Vec<String>) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.accept = AcceptSuggestions::High;
    for s in sets {
        req.overrides
            .push(engine::overrides::parse_set(&s).unwrap());
    }
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "rnm-status");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

#[test]
fn codes_are_in_the_catalogue() {
    for (s, sev) in [
        ("RNM-NO-BASE", Severity::Warning),
        ("RNM-IGNORED-EYE", Severity::Warning),
        ("RNM-MASK-ORPHAN", Severity::Warning),
        ("RNM-AO-MISSING", Severity::Info),
        ("RNM-LAYER-SHADOWED", Severity::Warning),
    ] {
        let c = Code::parse(s).unwrap();
        assert_eq!((c.as_str(), c.severity()), (s, sev));
    }
}

#[test]
fn layer_never_writes_alone() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_n.png",
        "iron_bump.png",
        "iron_detailnormmask.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    for name in ["iron_bump.png", "iron_detailnormmask.png"] {
        let j = p.jobs.iter().find(|j| j.input.ends_with(name)).expect(name);
        assert!(
            matches!(j.status, JobStatus::NormalLayer { .. }),
            "{name}: {:?}",
            j.status
        );
    }
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "rnm-status-run");
    Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    let mut written: Vec<String> = std::fs::read_dir(d.path().join("out"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".dds"))
        .collect();
    written.sort();
    assert_eq!(written, vec!["iron.dds", "iron_n.dds"]);
    assert!(sink.take().iter().any(|e| matches!(
        &e.kind,
        EventKind::FileSkipped {
            kind: SkipKind::NormalLayer,
            ..
        }
    )));
}

/// Note W2: an explicit sub-type converts a layer-named file alone.
#[test]
fn explicit_subtype_converts_a_layer_named_file() {
    let d = tempfile::tempdir().unwrap();
    png(d.path(), "cloth_bent.png");
    let p = plan_dir(d.path(), vec!["file:cloth_bent.png.subtype=diffuse".into()]);
    let j = p
        .jobs
        .iter()
        .find(|j| j.input.ends_with("cloth_bent.png"))
        .unwrap();
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert!(!matches!(j.status, JobStatus::NormalLayer { .. }));
}
