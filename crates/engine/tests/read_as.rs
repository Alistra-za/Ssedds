//! M6 Track F3b: `[inputs]` aliases in `sdc.toml` read a source under another name; the file is never touched.
use engine::api::*;
use engine::codes::Code;
use engine::config::{Subtype, Target};
use engine::events::*;
use engine::policy::ResolvedJob;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn rgb(dir: &Path, rel: &str, c: [u8; 3]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([c[0].wrapping_add(x as u8), c[1], c[2]]))
        .save(p)
        .unwrap();
}

/// `in/armor/wall.png` (a diffuse), `in/armor/wallnormal.png` (a normal map saved without `_n`), and
/// `in/armor/other.png`. `toml` is written to `in/sdc.toml`.
fn fixture(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("in");
    rgb(&r, "armor/wall.png", [90, 60, 30]);
    rgb(&r, "armor/wallnormal.png", [128, 128, 255]);
    rgb(&r, "armor/other.png", [40, 80, 20]);
    std::fs::write(r.join("sdc.toml"), toml).unwrap();
    d
}

fn req(d: &TempDir) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r
}

fn plan_events(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| slash(&j.output).ends_with(tail))
        .unwrap_or_else(|| panic!("no job writing {tail}"))
}

fn warnings(events: &[EngineEvent], code: Code) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == code => Some(w.message.clone()),
            _ => None,
        })
        .collect()
}

const WALL_N: &str = "[inputs.\"armor/wallnormal.png\"]\nas = \"armor/wall_n.png\"\n";

#[test]
fn an_alias_renames_the_file_for_grouping_kind_and_output() {
    let d = fixture(WALL_N);
    let (p, events) = plan_events(req(&d));
    let j = job(&p, "out/armor/wall_n.dds");
    assert!(
        slash(&j.input).ends_with("in/armor/wallnormal.png"),
        "decoded from the real file"
    );
    assert_eq!(j.game_path, "armor/wall_n.png");
    assert_eq!(j.profile.subtype, Subtype::Normal);
    assert_eq!(
        job(&p, "out/armor/wall.dds").set_key,
        j.set_key,
        "joins the wall set"
    );
    assert!(!p
        .jobs
        .iter()
        .any(|j| slash(&j.output).ends_with("wallnormal.dds")));
    assert!(warnings(&events, Code::InAliasInvalid).is_empty());
    assert!(warnings(&events, Code::InAliasStale).is_empty());
    assert!(
        d.path().join("in/armor/wallnormal.png").exists(),
        "never renamed on disk"
    );
}

#[test]
fn the_key_matches_without_extension_and_in_any_case_and_the_alias_may_drop_it() {
    let d = fixture("[inputs.\"Armor/WallNormal\"]\nas = 'armor\\wall_n'\n");
    let (p, events) = plan_events(req(&d));
    assert_eq!(
        job(&p, "out/armor/wall_n.dds").profile.subtype,
        Subtype::Normal
    );
    assert!(warnings(&events, Code::InAliasInvalid).is_empty());
}

#[test]
fn a_folder_alias_moves_the_output_and_the_game_path() {
    let d = fixture("[inputs.\"armor/other.png\"]\nas = \"clutter/other.png\"\n");
    let (p, _) = plan_events(req(&d));
    let j = job(&p, "out/clutter/other.dds");
    assert_eq!(j.game_path, "clutter/other.png");
}

#[test]
fn files_settings_stay_keyed_by_the_real_file() {
    let d = fixture(&format!(
        "{WALL_N}[files.\"armor/wallnormal.png\"]\nalpha.threshold = 100\n"
    ));
    let (p, events) = plan_events(req(&d));
    let j = job(&p, "out/armor/wall_n.dds");
    assert_eq!(j.profile.alpha.threshold, 100);
    assert_eq!(j.provenance["alpha.threshold"].to_string(), "project_file");
    assert!(warnings(&events, Code::CfgFileUnmatched).is_empty());
}

#[test]
fn a_rule_glob_sees_the_alias() {
    let d = fixture(&format!(
        "{WALL_N}[[rules]]\nmatch = {{ glob = \"**/*_n.png\" }}\noverrides = {{ alpha = {{ threshold = 77 }} }}\n"
    ));
    let (p, _) = plan_events(req(&d));
    assert_eq!(job(&p, "out/armor/wall_n.dds").profile.alpha.threshold, 77);
}

#[test]
fn refused_aliases_leave_the_file_as_it_is() {
    for (alias, why) in [
        ("armor/wall_n.tga", "own extension"),
        ("../wall_n.png", ".."),
        ("C:/wall_n.png", "relative"),
        ("armor/other.png", "already another file"),
        ("", "names no file"),
    ] {
        let d = fixture(&format!(
            "[inputs.\"armor/wallnormal.png\"]\nas = '{alias}'\n"
        ));
        let (p, events) = plan_events(req(&d));
        let w = warnings(&events, Code::InAliasInvalid);
        assert_eq!(w.len(), 1, "{alias}: {w:?}");
        assert!(w[0].contains(why), "{alias}: {}", w[0]);
        job(&p, "out/armor/wallnormal.dds");
    }
}

#[test]
fn two_aliases_onto_one_name_refuse_the_second() {
    let d = fixture(concat!(
        "[inputs.\"armor/other.png\"]\nas = \"armor/wall_n.png\"\n",
        "[inputs.\"armor/wallnormal.png\"]\nas = \"armor/wall_n.png\"\n"
    ));
    let (p, events) = plan_events(req(&d));
    assert_eq!(warnings(&events, Code::InAliasInvalid).len(), 1);
    assert_eq!(
        p.jobs
            .iter()
            .filter(|j| slash(&j.output).ends_with("armor/wall_n.dds"))
            .count(),
        1
    );
}

#[test]
fn a_key_that_matches_no_file_is_stale() {
    let d = fixture("[inputs.\"armor/gone.png\"]\nas = \"armor/gone_n.png\"\n");
    let (_, events) = plan_events(req(&d));
    let w = warnings(&events, Code::InAliasStale);
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("armor/gone.png"));
}

#[test]
fn no_inputs_table_changes_nothing() {
    let d = fixture("");
    let (p, _) = plan_events(req(&d));
    job(&p, "out/armor/wallnormal.dds");
    assert!(p
        .jobs
        .iter()
        .all(|j| !slash(&j.output).ends_with("wall_n.dds")));
}

#[test]
fn a_user_config_cannot_hold_inputs() {
    let d = fixture("");
    let user = d.path().join("user.toml");
    std::fs::write(&user, WALL_N).unwrap();
    let mut r = req(&d);
    r.user_config = Some(user);
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "t");
    let Err(e) = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
    else {
        panic!("a user config with [inputs] must be refused");
    };
    assert!(e.to_string().contains("[inputs] is only valid"), "{e}");
}
