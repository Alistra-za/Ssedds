//! M3a Task 3: the `[files]` table of `sdc.toml` (DECISIONS N2) and the in-memory project config (N4).
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::discover::{InputFile, InputKind};
use engine::events::*;
use engine::overrides::parse_set;
use engine::overrides::project_rel;
use engine::policy::{JobStatus, ResolvedJob};
use engine::presets::ConfigFile;
use image::{ImageBuffer, Luma, Rgb};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

fn rgb(dir: &Path, rel: &str, w: u32, c: [u8; 3]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, _| Rgb([c[0].wrapping_add(x as u8), c[1], c[2]]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

/// `in/` with two `rock01_n.png` in two folders, a diffuse, two 48 px (NPOT) files, and roughness + AO so cs packs
/// `landscape/rock01_rmaos`. `toml` is written to `in/sdc.toml`.
fn fixture(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("in");
    rgb(&r, "landscape/rock01_n.png", 64, [128, 128, 255]);
    rgb(&r, "other/rock01_n.png", 64, [128, 128, 255]);
    rgb(&r, "landscape/rock01.png", 64, [90, 60, 30]);
    rgb(&r, "landscape/odd.png", 48, [90, 60, 30]);
    rgb(&r, "other/odd.png", 48, [90, 60, 30]);
    gray(&r, "landscape/rock01_roughness.png", 120);
    gray(&r, "landscape/rock01_ao.png", 200);
    std::fs::write(r.join("sdc.toml"), toml).unwrap();
    d
}

fn req(d: &TempDir) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r
}

fn plan_events(req: BatchRequest) -> (Result<Plan, EngineError>, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default());
    (p, sink.take())
}

fn plan(req: BatchRequest) -> Plan {
    plan_events(req).0.unwrap()
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| {
            j.output
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(tail)
        })
        .unwrap_or_else(|| panic!("no job writing {tail}"))
}

fn prov(j: &ResolvedJob, field: &str) -> String {
    j.provenance[field].to_string()
}

#[test]
fn files_entries_set_one_file_and_lose_to_the_command_line() {
    let d = fixture("[files.\"landscape/rock01_n.png\"]\nalpha.threshold = 100\n");
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01_n.dds");
    assert_eq!(j.profile.alpha.threshold, 100);
    assert_eq!(prov(j, "alpha.threshold"), "project_file");
    assert_ne!(job(&p, "other/rock01_n.dds").profile.alpha.threshold, 100);
    let mut r = req(&d);
    r.overrides = vec![parse_set("file:**/rock01_n.png.alpha.threshold=150").unwrap()];
    let p = plan(r);
    let j = job(&p, "landscape/rock01_n.dds");
    assert_eq!(j.profile.alpha.threshold, 150);
    assert_eq!(prov(j, "alpha.threshold"), "user");
}

#[test]
fn a_files_subtype_decides_the_file() {
    let d = fixture("[files.\"landscape/rock01.png\"]\nsubtype = \"diffuse\"\n");
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(prov(j, "subtype"), "project_file");
    assert_eq!(j.status, JobStatus::Ready);
}

#[test]
fn files_allow_acknowledges_only_that_file() {
    let d = fixture(concat!(
        "[files.\"landscape/odd.png\"]\nsubtype = \"diffuse\"\nallow = [\"DIM-NPOT\"]\n",
        "[files.\"other/odd.png\"]\nsubtype = \"diffuse\"\n"
    ));
    let p = plan(req(&d));
    assert_eq!(job(&p, "landscape/odd.dds").status, JobStatus::Ready);
    assert!(matches!(
        &job(&p, "other/odd.dds").status,
        JobStatus::Blocked { codes } if codes.contains(&Code::DimNpot)
    ));
}

#[test]
fn a_packed_output_is_keyed_without_extension() {
    let d = fixture("[files.\"landscape/rock01_rmaos\".packing.fill]\nmetallic = 255\n");
    let p = plan(req(&d));
    assert_eq!(
        prov(job(&p, "landscape/rock01_rmaos.dds"), "pack.g"),
        "fill:255"
    );
}

#[test]
fn an_unmatched_files_key_is_reported_once_and_kept() {
    let d = fixture("target = \"cpm\"\n[files.\"landscape/rock01_rmaos\"]\nalpha.threshold = 90\n");
    let mut r = req(&d);
    r.target = None;
    let (p, ev) = plan_events(r);
    assert!(p.is_ok());
    let hits: Vec<&EngineEvent> = ev
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::CfgFileUnmatched))
        .collect();
    assert_eq!(hits.len(), 1);
    let EventKind::Warning(w) = &hits[0].kind else {
        unreachable!()
    };
    assert_eq!(w.data["key"], "landscape/rock01_rmaos");
    // The file is not rewritten by planning.
    let text = std::fs::read_to_string(d.path().join("in/sdc.toml")).unwrap();
    assert!(text.contains("landscape/rock01_rmaos"));
}

#[test]
fn the_inline_config_replaces_the_file_on_disk() {
    let d = fixture("[files.\"landscape/rock01_n.png\"]\nalpha.threshold = 100\n");
    let mut r = req(&d);
    let mut cfg = ConfigFile::default();
    cfg.files.insert(
        "landscape/rock01_n.png".into(),
        serde_json::json!({ "alpha": { "threshold": 150 } }),
    );
    r.project_config_inline = Some(cfg);
    let p = plan(r);
    assert_eq!(
        job(&p, "landscape/rock01_n.dds").profile.alpha.threshold,
        150
    );
}

#[test]
fn bad_files_entries_are_config_errors_and_user_config_files_are_rejected() {
    let config_err = |toml: &str| match plan_events(req(&fixture(toml))).0 {
        Err(EngineError::Config(m)) => m,
        other => panic!("expected a config error, got {:?}", other.map(|_| ())),
    };
    assert!(config_err("[files.\"x.png\"]\ncolour = \"srgb\"\n").contains("x.png"));
    assert!(config_err("[files.\"x.png\"]\nallow = [\"NOT-A-CODE\"]\n").contains("NOT-A-CODE"));
    let d = fixture("");
    let user = d.path().join("user.toml");
    std::fs::write(&user, "[files.\"a.png\"]\nalpha.threshold = 90\n").unwrap();
    let mut r = req(&d);
    r.user_config = Some(user);
    match plan_events(r).0 {
        Err(EngineError::Config(m)) => {
            assert!(m.contains("[files] is only valid in a project sdc.toml"))
        }
        other => panic!("expected a config error, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn matched_keys_are_not_reported() {
    let d = fixture(concat!(
        "[files.\"landscape/rock01_rmaos\".packing.fill]\nmetallic = 255\n",
        "[files.\"LANDSCAPE/rock01_n.png\"]\nalpha.threshold = 100\n",
        "[files.\"landscape/rock01_roughness.png\"]\nalpha.threshold = 100\n"
    ));
    let (p, ev) = plan_events(req(&d));
    p.unwrap();
    assert!(!ev
        .iter()
        .any(|e| matches!(&e.kind, EventKind::Warning(w) if w.code == Code::CfgFileUnmatched)));
}

#[test]
fn project_root_follows_the_config_source() {
    let d = fixture("");
    let mut r = req(&d);
    assert_eq!(project_root(&r), Some(d.path().join("in")));
    r.project_config_inline = Some(ConfigFile::default());
    assert_eq!(project_root(&r), Some(d.path().join("in")));
    let mut r = req(&d);
    r.project_config = Some(d.path().join("elsewhere/sdc.toml"));
    assert_eq!(project_root(&r), Some(d.path().join("elsewhere")));
    let r = BatchRequest::new(
        vec![d.path().join("in/landscape/rock01.png")],
        d.path().join("out"),
        Target::Cs,
    );
    assert_eq!(project_root(&r), None);
}

#[test]
fn a_relative_config_path_roots_at_the_current_directory() {
    let cwd = std::path::absolute(".").unwrap();
    let mut r = BatchRequest::new(
        vec![PathBuf::from("textures")],
        PathBuf::from("out"),
        Target::Cs,
    );
    r.project_config = Some(PathBuf::from("sdc.toml"));
    assert_eq!(project_root(&r), Some(cwd.clone()));
    r.project_config = Some(PathBuf::from("proj/sdc.toml"));
    assert_eq!(project_root(&r), Some(cwd.join("proj")));
}

#[test]
fn project_rel_resolves_a_relative_input_against_an_absolute_root() {
    let cwd = std::path::absolute(".").unwrap();
    let input = |p: &str| InputFile {
        path: PathBuf::from(p),
        rel: PathBuf::from(Path::new(p).file_name().unwrap()),
        game_path: String::new(),
        output: PathBuf::new(),
        kind: InputKind::Image,
        real_rel: None,
    };
    let rel = |root: &Path, p: &str| project_rel(root, &input(p));
    assert_eq!(
        rel(&cwd, "landscape/rock01_n.png").as_deref(),
        Some("landscape/rock01_n.png")
    );
    assert_eq!(rel(&cwd, "rock.png").as_deref(), Some("rock.png"));
    // A relative root works the same way, and an input outside the root is not under it.
    assert_eq!(rel(Path::new("."), "a/b.png").as_deref(), Some("a/b.png"));
    assert_eq!(rel(&cwd.join("proj"), "landscape/rock01_n.png"), None);
    assert_eq!(
        rel(&cwd.join("proj"), "proj/x/y.png").as_deref(),
        Some("x/y.png")
    );
}

/// DECISIONS N2 / spec §5.3: a config rule < a `[files]` entry < a command-line `--set`, all on one field of one file.
#[test]
fn a_rule_loses_to_a_files_entry_which_loses_to_the_command_line() {
    let rule = "[[rules]]\nmatch = { glob = \"**/rock01_n.png\" }\noverrides = { alpha = { threshold = 80 } }\n";
    let files = "[files.\"landscape/rock01_n.png\"]\nalpha.threshold = 100\n";
    let set = || parse_set("file:landscape/rock01_n.png.alpha.threshold=150").unwrap();
    let threshold = |p: &Plan| {
        let j = job(p, "landscape/rock01_n.dds");
        (j.profile.alpha.threshold, prov(j, "alpha.threshold"))
    };

    // The rule alone.
    let d = fixture(rule);
    let (value, source) = threshold(&plan(req(&d)));
    assert_eq!(value, 80);
    assert!(source.starts_with("rule:"), "{source}");

    // The rule and a [files] entry: the entry wins.
    let d = fixture(&format!("{rule}\n{files}"));
    assert_eq!(threshold(&plan(req(&d))), (100, "project_file".to_string()));

    // All three: the command line wins.
    let mut r = req(&d);
    r.overrides = vec![set()];
    assert_eq!(threshold(&plan(r)), (150, "user".to_string()));

    // The command line over the rule alone, for completeness.
    let d = fixture(rule);
    let mut r = req(&d);
    r.overrides = vec![set()];
    assert_eq!(threshold(&plan(r)), (150, "user".to_string()));
}

/// Carried fix 3: a `[files]` sub-type beats a rule's sub-type, and that rule's overrides then do not apply; a
/// `[files]` field beats a rule's field for the same sub-type.
#[test]
fn files_entries_beat_rules_and_drop_rules_for_another_subtype() {
    let d = fixture(concat!(
        "[[rules]]\nmatch = { glob = \"**/landscape/rock01.png\" }\nsubtype = \"glow\"\noverrides = { alpha = { threshold = 90 } }\n",
        "[files.\"landscape/rock01.png\"]\nsubtype = \"diffuse\"\n",
    ));
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(prov(j, "subtype"), "project_file");
    assert_eq!(j.profile.alpha.threshold, 127);
    let d = fixture(concat!(
        "[[rules]]\nmatch = { glob = \"**/landscape/rock01.png\" }\nsubtype = \"diffuse\"\noverrides = { alpha = { threshold = 90 } }\n",
        "[files.\"landscape/rock01.png\"]\nalpha.threshold = 100\n",
    ));
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(j.profile.alpha.threshold, 100);
    assert_eq!(prov(j, "alpha.threshold"), "project_file");
}
