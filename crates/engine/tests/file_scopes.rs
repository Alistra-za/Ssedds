use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{JobStatus, ResolvedJob};
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

/// A `w`×`w` RGB PNG with no alpha at `dir/rel`.
fn png(dir: &Path, rel: &str, w: u32) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90]))
        .save(p)
        .unwrap();
}

fn plan(req: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

fn job<'a>(p: &'a Plan, rel: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(rel))
        .unwrap_or_else(|| panic!("no job for {rel}"))
}

/// DIM-NPOT is blocking; acknowledging it for `a/x.png` must leave `b/x.png` blocked.
#[test]
fn a_per_file_allow_acknowledges_one_file_only() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("in");
    png(&root, "a/x.png", 48);
    png(&root, "b/x.png", 48);
    let mut req = BatchRequest::new(vec![root.clone()], d.path().join("out"), Target::Cs);
    req.recursive = true;
    req.overrides = vec![parse_set("file:**/x.png.subtype=diffuse").unwrap()];
    req.file_allow
        .insert("a/x.png".into(), [Code::DimNpot].into_iter().collect());
    let p = plan(req);
    assert_eq!(job(&p, "a/x.png").status, JobStatus::Ready);
    assert!(matches!(
        &job(&p, "b/x.png").status,
        JobStatus::Blocked { codes } if codes.contains(&Code::DimNpot)
    ));
}
