//! Note FC1: the GUI's automatic decisions (High suggestions, mesh-vote sub-types, mesh-alpha verdicts) are recorded
//! in the project as one undo step with one Problems row; the grass remedy writes `subtype = "complex_grass"`; a
//! missing preset blocks the export with a row.
mod support;
use engine::config::{AlphaPolicy, Subtype, Target};
use engine::presets::ConfigFile;
use engine::shader::{MeshAlpha, Shader};
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::bottom::{problems, At, Fix, Level, Problem};
use sdc_gui::model::AppModel;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);

fn rgb(root: &Path, rel: &str, px: [u8; 3]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb(px))
        .save(p)
        .unwrap();
}

/// A colour map whose alpha varies (a ramp): without evidence its alpha is asked.
fn varying(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Rgba([90u8, 60, 30, (x * 16) as u8]))
        .save(p)
        .unwrap();
}

fn auto_row(m: &AppModel) -> Option<Problem> {
    problems(m)
        .iter()
        .find(|p| p.level == Level::Info && p.text.contains("set automatically"))
        .cloned()
}

fn auto_steps(m: &AppModel) -> usize {
    m.history
        .undo
        .iter()
        .filter(|c| c.label.ends_with("automatically"))
        .count()
}

fn text(m: &AppModel) -> String {
    m.project.as_ref().unwrap().text()
}

fn subtype(m: &AppModel, key: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(key, "subtype")
}

/// What `sdc convert` (CLI default `--accept-suggestions none`, no scan evidence) plans from the document's text.
fn cli_plan(root: &Path, toml: &str) -> engine::api::Plan {
    use engine::api::*;
    use engine::events::{new_batch_id, NullSink, Reporter};
    let mut req = BatchRequest::new(vec![root.to_path_buf()], root.join("../cli"), Target::Cs);
    req.recursive = true;
    req.project_config_inline = Some(ConfigFile::parse(toml, "sdc.toml").unwrap());
    let rep = Reporter::new(std::sync::Arc::new(NullSink), new_batch_id());
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

fn cli_job<'a>(p: &'a engine::api::Plan, name: &str) -> &'a engine::policy::ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.ends_with(name))
        .unwrap_or_else(|| panic!("no job {name}"))
}

#[test]
fn auto_accepted_types_are_recorded_and_undoable() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    std::fs::write(root.join("sdc.toml"), "# my notes\n").unwrap();
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    let job = m.item("a/rock_n.png").unwrap().job.clone().unwrap();
    assert_eq!(job.profile.subtype, Subtype::Normal);
    assert!(m.is_dirty(), "the recording is an unsaved change");
    assert_eq!(subtype(&m, "a/rock_n.png"), Some(json!("normal")));
    let t = text(&m);
    assert_eq!(t.lines().next(), Some("# my notes"), "{t}");
    assert!(t.contains("subtype = \"normal\""), "{t}");
    // The plan that the recording started finds nothing new: exactly one step.
    assert_eq!(auto_steps(&m), 1, "{:?}", m.history.undo);
    assert_eq!(m.undo_label(), Some("Set 1 type automatically"));
    let row = auto_row(&m).expect("the combined row");
    assert_eq!(row.text, "1 type set automatically (High confidence)");
    assert_eq!(
        (row.at.clone(), row.fix.clone()),
        (At::Project, Some(Fix::UndoLast))
    );
    assert!(
        row.detail.as_deref().unwrap_or("").contains("a/rock_n.png"),
        "{row:?}"
    );
    // Undo takes the entry out, and the next plan does not record it again.
    m.undo();
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/rock_n.png"), None);
    assert_eq!(auto_steps(&m), 0);
    assert!(m.can_redo(), "the redo step survives the plan");
    assert!(auto_row(&m).is_none());
    assert_eq!(text(&m).lines().next(), Some("# my notes"));
    // Redo puts it back, and the plan after it records nothing new.
    m.redo();
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/rock_n.png"), Some(json!("normal")));
    assert_eq!(auto_steps(&m), 1);
    assert!(!m.can_redo());
}

/// A user's own choice of the value the plan recorded is theirs: the plan does not change it again.
#[test]
fn choosing_the_recorded_type_makes_it_the_users() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    let mut m = support::opened_as_the_app(root, d.path().join("export"));
    let job = |m: &AppModel| m.item("a/rock_n.png").unwrap().job.clone().unwrap();
    assert!(
        sdc_gui::model::fields::auto_subtype_reason(&job(&m)).is_some(),
        "still (auto)"
    );
    m.set_field("a/rock_n.png", "subtype", json!("normal"));
    assert!(m.wait_idle(WAIT));
    assert_eq!(
        job(&m).provenance["subtype"].to_string(),
        "project_file",
        "the user's own value"
    );
}

/// Note FB5: a crash restore's toast can undo it while it is the top step, so nothing is recorded over it.
#[test]
fn nothing_is_recorded_over_a_crash_restore() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    let file = root.join("sdc.toml");
    std::fs::write(&file, "target = \"vanilla\"\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))
        .unwrap();
    std::fs::write(
        sdc_gui::crash_log::recovery_path(&file),
        "target = \"cs\"\n",
    )
    .unwrap();
    let mut m = AppModel::new(
        sdc_gui::settings::GuiSettings::default(),
        None,
        support::session(),
    );
    m.open_folder(root);
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.undo_label(), Some("Restore crash edits"));
    assert_eq!(subtype(&m, "a/rock_n.png"), None);
    m.undo_crash_restore();
    assert!(m.wait_idle(WAIT));
    // The toast's undo took the restore back whole; the plan of the saved text then records as an open does.
    let t = text(&m);
    assert!(
        t.starts_with("target = \"vanilla\"\n") && !t.contains("\"cs\""),
        "{t}"
    );
    assert_eq!(m.undo_label(), Some("Set 1 type automatically"));
}

/// The leading comments of a comment-only `sdc.toml` stay first when an entry is added, and come back when it goes.
#[test]
fn a_comment_only_file_keeps_its_first_line() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc.toml"), "# my notes\n").unwrap();
    let mut p = sdc_gui::project::ProjectDoc::open(d.path());
    p.set_file_value("a.png", "subtype", &json!("normal"));
    assert!(p.text().starts_with("# my notes\n[files."), "{}", p.text());
    p.clear_file_value("a.png", "subtype");
    assert_eq!(p.text(), "# my notes\n");
}

/// Review Focus 1: a sub-type the user wrote is never touched; only the other file is recorded.
#[test]
fn an_explicit_subtype_is_never_touched() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    rgb(&root, "a/wall_n.png", [128, 128, 255]);
    let mine = "# mine\n[files.\"a/rock_n.png\"]\nsubtype = \"msn\" # keep\n";
    std::fs::write(root.join("sdc.toml"), mine).unwrap();
    let m = support::opened_as_the_app(root, d.path().join("export"));
    assert_eq!(subtype(&m, "a/rock_n.png"), Some(json!("msn")));
    assert_eq!(subtype(&m, "a/wall_n.png"), Some(json!("normal")));
    let t = text(&m);
    assert!(t.starts_with(mine), "{t}");
    let row = auto_row(&m).expect("the combined row");
    let detail = row.detail.unwrap_or_default();
    assert!(
        detail.contains("a/wall_n.png") && !detail.contains("a/rock_n.png"),
        "{detail}"
    );
}

/// X10/X11: the grass remedy writes the sub-type the CLI reads, with no GRAS evidence.
#[test]
fn grass_remedy_writes_the_subtype() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "plants/g.png", [60, 120, 40]);
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    m.tag_as_grass("plants/g.png");
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "plants/g.png"), Some(json!("complex_grass")));
    assert_eq!(m.undo_label(), Some("Tag as grass"));
    let p = cli_plan(&root, &text(&m));
    assert_eq!(cli_job(&p, "g.png").profile.subtype, Subtype::ComplexGrass);
    // The arm Task 10's row carries.
    assert_eq!(
        Fix::TagAsGrass("plants/g.png".into()),
        Fix::TagAsGrass("plants/g.png".into())
    );
}

/// U5, CN59: a mesh-vote sub-type and a mesh-alpha verdict are recorded with the High types, in the one row; the CLI
/// then plans the same sub-type and alpha from the text with no evidence.
#[test]
fn evidence_decisions_are_recorded() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "landscape/grass/g.png", [60, 120, 40]);
    varying(&root, "a/wall.png");
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    // Before the scan: the colour map is recorded as what its name says.
    assert_eq!(
        subtype(&m, "landscape/grass/g.png"),
        Some(json!("landscape_diffuse"))
    );
    // Stub evidence, as a scan that finishes later gives it: kept by the read while the store is unsettled.
    m.mesh_votes = [(
        "textures/landscape/grass/g".to_string(),
        vec![Shader::PbrGrass],
    )]
    .into();
    let tested = MeshAlpha {
        blend: false,
        test: true,
        threshold: 100,
    };
    m.mesh_alpha = [("textures/a/wall".to_string(), vec![tested])].into();
    m.scan_store_unsettled = true;
    m.replan();
    assert!(m.wait_idle(WAIT));
    let g = m
        .item("landscape/grass/g.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_eq!(g.profile.subtype, Subtype::PbrAlbedo, "the vote decides");
    let w = m.item("a/wall.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (w.profile.alpha.policy, w.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 100)
    );
    let doc = m.project.as_ref().unwrap();
    assert_eq!(
        doc.file_value("landscape/grass/g.png", "subtype"),
        Some(json!("pbr_albedo"))
    );
    assert_eq!(
        doc.file_value("a/wall.png", "alpha.policy"),
        Some(json!("cutout"))
    );
    assert_eq!(
        doc.file_value("a/wall.png", "alpha.threshold"),
        Some(json!(100))
    );
    // The recorded type stays automatic in the GUI ("(auto)", with its reason), so the scan could re-type it.
    assert!(sdc_gui::model::fields::auto_subtype_reason(&g).is_some());
    let detail = auto_row(&m)
        .expect("the combined row")
        .detail
        .unwrap_or_default();
    assert!(
        detail.contains("landscape/grass/g.png: landscape_diffuse → pbr_albedo")
            && detail.contains("a/wall.png: cutout"),
        "{detail}"
    );
    let p = cli_plan(&root, &text(&m));
    assert_eq!(cli_job(&p, "g.png").profile.subtype, Subtype::PbrAlbedo);
    let cw = cli_job(&p, "wall.png");
    assert_eq!(
        (cw.profile.alpha.policy, cw.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 100)
    );
}

/// AM14: one rule for both tools. The GUI still plans (on the fallback) but the export is blocked by a row.
#[test]
fn a_missing_preset_blocks_export_with_a_row() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    std::fs::write(root.join("sdc.toml"), "preset = \"ghost\"\n").unwrap();
    let m = support::opened_as_the_app(root, d.path().join("export"));
    assert!(!m.items.is_empty(), "the window is not empty");
    assert_eq!(
        m.review_gate(),
        Err("Preset ghost is not installed".to_string())
    );
    assert!(!m.export_is_clean());
    let rows = problems(&m);
    let row = rows
        .iter()
        .find(|p| p.text == "Preset ghost is not installed")
        .expect("the row");
    assert_eq!(
        (row.level, row.at.clone(), row.fix.clone()),
        (Level::Blocks, At::Project, Some(Fix::ChoosePreset))
    );
    assert!(
        !m.banner
            .clone()
            .unwrap_or_default()
            .contains("not found: using"),
        "{:?}",
        m.banner
    );
}
