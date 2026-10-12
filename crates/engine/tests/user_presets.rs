//! M6 Track D spec §3: the user presets folder through `Engine::plan` (`BatchRequest::user_presets`).
use engine::api::*;
use engine::codes::Code;
use engine::config::{Format, Target};
use engine::events::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/wall.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([x as u8 * 7, y as u8 * 7, 90]))
        .save(p)
        .unwrap();
    d
}

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn request(d: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = engine::policy::AcceptSuggestions::High;
    r.overrides = vec![engine::overrides::parse_set("subtype=diffuse").unwrap()];
    r
}

fn plan(req: BatchRequest) -> (Result<Plan, EngineError>, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default());
    (p, sink.take())
}

const MINE: &str = "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n";

/// A folder preset is selectable with no flag; a bad one is skipped with CFG-PRESET-SKIPPED and the run continues.
#[test]
fn folder_presets_are_selectable_and_bad_ones_are_skipped() {
    let d = fixture();
    let dir = d.path().join("presets");
    write(&dir, "mine.toml", MINE);
    write(
        &dir,
        "zz_bad.toml",
        "schema_version = 1\nid = \"bad\"\nversion = \"1\"\nbase = \"nope\"\n",
    );
    let mut req = request(d.path());
    req.user_presets = Some(dir.clone());
    req.preset = Some("mine".into());
    let (p, events) = plan(req);
    let p = p.unwrap();
    assert_eq!(p.preset_id, "mine");
    assert_eq!(p.jobs[0].profile.format, Format::Bc1);
    assert!(p.registry.source("mine").unwrap().ends_with("mine.toml"));
    let skipped: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::CfgPresetSkipped => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].data["file"]
        .as_str()
        .unwrap()
        .ends_with("zz_bad.toml"));
    // No folder given: nothing user-side is loaded.
    let mut req = request(d.path());
    req.preset = Some("mine".into());
    assert!(matches!(plan(req).0, Err(EngineError::Usage(_))));
}

/// Spec §3: a `--preset-file` whose id is already loaded is an error naming both files; the same file twice is not.
#[test]
fn a_preset_file_duplicate_is_an_error_and_the_same_file_is_not() {
    let d = fixture();
    let dir = d.path().join("presets");
    let folder_file = write(&dir, "mine.toml", MINE);
    let other = write(&d.path().join("elsewhere"), "mine.toml", MINE);
    let mut req = request(d.path());
    req.user_presets = Some(dir.clone());
    req.preset_files = vec![other];
    match plan(req).0 {
        Err(EngineError::Config(m)) => {
            assert!(m.contains("elsewhere") && m.contains("presets"), "{m}")
        }
        Ok(_) => panic!("expected a duplicate error"),
        Err(e) => panic!("{e:?}"),
    }
    let mut req = request(d.path());
    req.user_presets = Some(dir);
    req.preset_files = vec![folder_file];
    req.preset = Some("mine".into());
    assert!(plan(req).0.is_ok());
}
