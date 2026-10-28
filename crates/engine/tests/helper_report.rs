//! Helper textures Task 5 (Note DT13-DT14): one report per helper, and `if-changed` re-encodes the outputs a helper
//! fills when the helper or its read changes.
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::helper::report::HelperReport;
use engine::helper::{AlphaTarget, FillTarget, HelperRole};
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{ImageBuffer, Luma, Rgb};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const N: u32 = 64;
const DIFFUSE: &str = "file:**/x.png.subtype=diffuse";
/// A filled alpha needs a confirmed meaning to convert.
const BLEND: &str = "alpha.policy=blend";

fn at(d: &Path, name: &str) -> PathBuf {
    let p = d.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    p
}

fn rgb(d: &Path, name: &str) {
    ImageBuffer::from_fn(N, N, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90]))
        .save(at(d, name))
        .unwrap();
}

fn normal(d: &Path, name: &str) {
    ImageBuffer::from_fn(N, N, |x, _| Rgb([120 + (x % 16) as u8, 128, 255]))
        .save(at(d, name))
        .unwrap();
}

/// 16 px squares of `lo` and `hi`.
fn checker(d: &Path, name: &str, lo: u8, hi: u8) {
    ImageBuffer::from_fn(N, N, |x, y| {
        Luma([if (x / 16 + y / 16) % 2 == 0 { hi } else { lo }])
    })
    .save(at(d, name))
    .unwrap();
}

fn toml(d: &Path, text: &str) {
    std::fs::write(d.join("in/sdc.toml"), text).unwrap();
}

fn req(d: &Path, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.on_existing = OnExisting::IfChanged;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r
}

fn plan_ev(d: &Path, sets: &[&str]) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "helper-report");
    let p = Engine::new()
        .unwrap()
        .plan(req(d, sets), &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn plan(d: &Path, sets: &[&str]) -> Plan {
    plan_ev(d, sets).0
}

fn run(d: &Path, sets: &[&str]) -> BatchSummary {
    let rep = Reporter::new(Arc::new(VecSink::default()), "helper-report");
    let e = Engine::new().unwrap();
    let p = e.plan(req(d, sets), &rep, &CancelToken::default()).unwrap();
    e.run(p, &rep, &CancelToken::default()).unwrap()
}

fn report<'a>(p: &'a Plan, name: &str) -> &'a HelperReport {
    p.helpers
        .iter()
        .find(|h| h.path.ends_with(name))
        .unwrap_or_else(|| panic!("no report for {name}: {:#?}", p.helpers))
}

fn names(p: &Plan) -> Vec<String> {
    p.helpers
        .iter()
        .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn report_lists_every_helper_once() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_roughness.png", 40, 200);
    checker(d.path(), "x_ao.png", 100, 255);
    checker(d.path(), "x_opacity.png", 0, 255);
    normal(d.path(), "x_n.png");
    normal(d.path(), "x_detailnorm.png");
    let p = plan(d.path(), &[DIFFUSE]);
    assert_eq!(
        names(&p),
        [
            "x_ao.png",
            "x_detailnorm.png",
            "x_opacity.png",
            "x_roughness.png"
        ]
    );
    let rmaos = |channel| FillTarget::Packed {
        suffix: "rmaos",
        channel,
    };
    assert_eq!(report(&p, "x_roughness.png").fills, [rmaos(0)]);
    assert_eq!(report(&p, "x_ao.png").fills, [rmaos(2)]);
    let op = report(&p, "x_opacity.png");
    assert_eq!(op.fills, [FillTarget::Alpha(AlphaTarget::Diffuse)]);
    assert_eq!(op.role, HelperRole::Raw(RawMap::Opacity));
    assert!(op.outputs[0].ends_with("x.dds"), "{:?}", op.outputs);
    assert!(op
        .options
        .contains(&FillTarget::Alpha(AlphaTarget::Diffuse)));
    // A PBR set's `_n` is a PbrNormal: never an alpha target (Ruling 4).
    assert!(!op.options.contains(&FillTarget::Alpha(AlphaTarget::N)));
    assert!(op.options.contains(&rmaos(3)));
    let dn = report(&p, "x_detailnorm.png");
    assert_eq!(dn.fills, [FillTarget::Rnm(RnmTarget::N)]);
    assert_eq!(dn.fills[0].label("x"), "x_n");
    assert!(dn.outputs[0].ends_with("x_n.dds"), "{:?}", dn.outputs);
    assert!(dn.options.is_empty());
    for h in &p.helpers {
        assert!(!h.role_hand_set && !h.fills_hand_set, "{h:#?}");
        assert_eq!(h.not_used, None, "{h:#?}");
    }
}

#[test]
fn report_not_used_says_why() {
    let d = tempfile::tempdir().unwrap();
    checker(d.path(), "x_opacity.png", 0, 255);
    let p = plan(d.path(), &[]);
    let w = p
        .jobs
        .iter()
        .find(|j| j.input.ends_with("x_opacity.png"))
        .and_then(|j| j.warnings.iter().find(|w| w.code == Code::PackUnusedInput))
        .expect("x_opacity.png is reported unused");
    let h = report(&p, "x_opacity.png");
    assert!(h.fills.is_empty());
    assert_eq!(h.not_used.as_deref(), Some(w.message.as_str()));
}

#[test]
fn report_marks_hand_set() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_roughness.png", 40, 200);
    checker(d.path(), "x_ao.png", 100, 255);
    toml(
        d.path(),
        "[files.\"textures/a/x_roughness\"]\nhelper.role = \"glossiness\"\n\
         [files.\"textures/a/x_ao\"]\nhelper.fills = []\n",
    );
    let p = plan(d.path(), &[DIFFUSE]);
    let g = report(&p, "x_roughness.png");
    assert!(g.role_hand_set && !g.fills_hand_set, "{g:#?}");
    assert_eq!(g.role, HelperRole::Raw(RawMap::Glossiness));
    let ao = report(&p, "x_ao.png");
    assert!(!ao.role_hand_set && ao.fills_hand_set, "{ao:#?}");
    assert!(ao.fills.is_empty());
    assert_eq!(ao.not_used.as_deref(), Some("no targets chosen"));
}

#[test]
fn sets_without_helpers_report_nothing() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    normal(d.path(), "x_n.png");
    assert!(plan(d.path(), &[DIFFUSE]).helpers.is_empty());
}

#[test]
fn if_changed_reencodes_when_helper_changes() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (1, 0), "{s:?}");
    checker(d.path(), "x_opacity.png", 30, 220);
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (1, 0), "{s:?}");
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (0, 1), "{s:?}");
}

#[test]
fn if_changed_reencodes_when_read_changes() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_roughness.png", 40, 200);
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (2, 0), "{s:?}");
    toml(
        d.path(),
        "[files.\"textures/a/x_roughness\"]\nhelper.read = \"g\"\n",
    );
    // x.dds stays; only x_rmaos.dds reads the helper.
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (1, 1), "{s:?}");
    let s = run(d.path(), &[DIFFUSE, BLEND]);
    assert_eq!((s.converted, s.up_to_date), (0, 2), "{s:?}");
}

#[test]
fn helper_files_entry_is_not_unmatched() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    toml(
        d.path(),
        "[files.\"textures/a/x_opacity\"]\nhelper.invert = true\n",
    );
    let (p, ev) = plan_ev(d.path(), &[DIFFUSE]);
    assert!(p.jobs.iter().any(|j| j.fill.is_some()), "{:#?}", p.jobs);
    let unmatched: Vec<_> = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::CfgFileUnmatched))
        .collect();
    assert!(unmatched.is_empty(), "{unmatched:#?}");
}
