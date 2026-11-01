//! Note FC1: the GUI's automatic decisions (High suggestions, mesh-vote sub-types, mesh-alpha verdicts) are written
//! into the project as plain values, each recording its own undo step with one Problems row; a missing preset blocks
//! the export with a row.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use engine::config::{AlphaPolicy, Subtype, Target};
use engine::presets::ConfigFile;
use engine::shader::{MeshAlpha, Shader};
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::bottom::{problems, At, BottomTab, Fix, Level, Problem};
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

fn value(m: &AppModel, key: &str, field: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(key, field)
}

fn subtype(m: &AppModel, key: &str) -> Option<serde_json::Value> {
    value(m, key, "subtype")
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
    assert_eq!(
        job.provenance["subtype"].to_string(),
        "project_file",
        "a recorded value is a plain project value"
    );
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
        row.detail.as_deref().unwrap_or("").contains("rock_n.png: "),
        "{row:?}"
    );
    // The GUI and `sdc convert` plan the same value with the same provenance (the manifests match).
    let p = cli_plan(&root, &text(&m));
    assert_eq!(
        cli_job(&p, "rock_n.png").provenance["subtype"],
        job.provenance["subtype"]
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

/// Note FI11 (A17, D15): the row's hover names files and types in words and explains High confidence.
#[test]
fn the_automatic_row_explains_high_confidence() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    rgb(&root, "landscape/grass/g.png", [60, 120, 40]);
    let m = support::opened_as_the_app(root, d.path().join("export"));
    let detail = auto_row(&m).expect("the row").detail.unwrap_or_default();
    assert!(
        detail.ends_with("High confidence: the file name and the pixels agree on the type."),
        "{detail}"
    );
    // The file name and the type the picker shows, not the folder and the id.
    assert!(detail.contains("rock_n.png: normal"), "{detail}");
    assert!(detail.contains("g.png: landscape diffuse"), "{detail}");
    assert!(!detail.contains("textures/") && !detail.contains("a/rock_n.png"));
    for st in Subtype::ALL {
        let id = st.id().to_string();
        assert!(!id.contains('_') || !detail.contains(&id), "raw id {id}");
    }
}

/// R2: a recording a user's edit leads to is its own undo step; Undo takes back only the recording.
#[test]
fn a_recording_after_a_user_edit_is_its_own_step() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/x.png", [90, 60, 30]);
    rgb(&root, "a/x_mask.png", [200, 200, 200]);
    let toml = "[files.\"a/x_mask.png\"]\nhelper.role = \"opacity\"\n";
    std::fs::write(root.join("sdc.toml"), toml).unwrap();
    let mut m = support::opened_as_the_app(root, d.path().join("export"));
    assert_eq!(subtype(&m, "a/x_mask.png"), None, "a helper has no type");
    let steps = m.history.undo.len();
    m.set_field("a/x_mask.png", "helper.role", json!("none"));
    assert!(m.wait_idle(WAIT));
    let st = subtype(&m, "a/x_mask.png").expect("its type is recorded");
    assert_eq!(m.history.undo.len(), steps + 2, "{:?}", m.history.undo);
    assert_eq!(m.undo_label(), Some("Set 1 type automatically"));
    let row = auto_row(&m).expect("the row");
    assert_eq!(row.fix, Some(Fix::UndoLast));
    assert!(
        row.detail.as_deref().unwrap_or("").contains(&format!(
            "x_mask.png: {}",
            st.as_str().unwrap().replace('_', " ")
        )),
        "{row:?}"
    );
    m.undo();
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/x_mask.png"), None);
    assert_eq!(
        value(&m, "a/x_mask.png", "helper.role"),
        Some(json!("none"))
    );
    assert_eq!(m.history.undo.len(), steps + 1, "the user's edit stays");
    assert!(auto_row(&m).is_none(), "its Undo was for the undone step");
}

/// R5: deciding a diffuse as `pbr_albedo` switches its set's members, also one whose type the plan recorded on its
/// own and nobody changed.
#[test]
fn pbr_follow_retypes_an_untouched_recorded_member() {
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/iron.png", [120, 90, 60]);
    rgb(&root, "a/iron_n.png", [128, 128, 255]);
    let mut m = support::opened_as_the_app(root, d.path().join("export"));
    assert_eq!(subtype(&m, "a/iron_n.png"), Some(json!("normal")));
    m.decide("a/iron.png", Answer::Subtype(Subtype::PbrAlbedo));
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/iron.png"), Some(json!("pbr_albedo")));
    assert_eq!(subtype(&m, "a/iron_n.png"), Some(json!("pbr_normal")));
    // A type the user chose is theirs: it does not follow.
    m.undo();
    assert!(m.wait_idle(WAIT));
    m.set_field("a/iron_n.png", "subtype", json!("msn"));
    assert!(m.wait_idle(WAIT));
    m.decide("a/iron.png", Answer::Subtype(Subtype::PbrAlbedo));
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/iron_n.png"), Some(json!("msn")));
}

fn set_id(m: &AppModel, key_end: &str) -> String {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.key.ends_with(key_end))
        .unwrap_or_else(|| panic!("no set {key_end}"))
        .id
        .clone()
}

/// Note FC1 R5 (round 3): a shader pick re-types an untouched recorded member, and the new type is recorded, so the
/// saved text plans the same under `sdc convert`.
#[test]
fn a_shader_pick_records_the_member_type_it_re_types() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/eye.png", [120, 90, 60]);
    rgb(&root, "a/eye_n.png", [128, 128, 255]);
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    assert_eq!(subtype(&m, "a/eye_n.png"), Some(json!("normal")));
    let id = set_id(&m, "eye");
    m.set_shader(&id, Some(Shader::Eye));
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/eye_n.png"), Some(json!("msn")));
    let cli = cli_plan(&root, &text(&m));
    assert_eq!(cli_job(&cli, "eye_n.png").profile.subtype, Subtype::Msn);
}

/// Note FC1 R5 (round 3): going back to Auto re-types an untouched member too, also after a reopen (a new history).
#[test]
fn choosing_auto_re_types_an_untouched_member_after_a_reopen() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/eye.png", [120, 90, 60]);
    rgb(&root, "a/eye_n.png", [128, 128, 255]);
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    let id = set_id(&m, "eye");
    m.set_shader(&id, Some(Shader::Eye));
    assert!(m.wait_idle(WAIT));
    m.save().unwrap();
    m.open_folder(root.clone());
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/eye_n.png"), Some(json!("msn")));
    let id = set_id(&m, "eye");
    m.set_shader(&id, None);
    assert!(m.wait_idle(WAIT));
    assert_eq!(subtype(&m, "a/eye_n.png"), Some(json!("normal")));
    let cli = cli_plan(&root, &text(&m));
    assert_eq!(cli_job(&cli, "eye_n.png").profile.subtype, Subtype::Normal);
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
        detail.contains("wall_n.png") && !detail.contains("rock_n.png"),
        "{detail}"
    );
}

/// U5, CN59, R1: a mesh-alpha verdict is recorded like the High types; later evidence never rewrites a recorded
/// value; the CLI plans the same from the text with no evidence.
#[test]
fn evidence_decisions_are_recorded_and_recorded_values_stay() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "landscape/grass/g.png", [60, 120, 40]);
    varying(&root, "a/wall.png");
    let mut m = support::opened_as_the_app(root.clone(), d.path().join("export"));
    // Before the scan: the colour maps are recorded as what their names say; the wall's alpha is still asked.
    assert_eq!(
        subtype(&m, "landscape/grass/g.png"),
        Some(json!("landscape_diffuse"))
    );
    assert_eq!(value(&m, "a/wall.png", "alpha.policy"), None);
    let steps = m.history.undo.len();
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
    // R1: the recorded sub-type is the project's; the vote does not rewrite it.
    assert_eq!(
        subtype(&m, "landscape/grass/g.png"),
        Some(json!("landscape_diffuse"))
    );
    let w = m.item("a/wall.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (w.profile.alpha.policy, w.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 100)
    );
    assert_eq!(
        value(&m, "a/wall.png", "alpha.policy"),
        Some(json!("cutout"))
    );
    assert_eq!(value(&m, "a/wall.png", "alpha.threshold"), Some(json!(100)));
    // Its own step and row; the count is of values, not only types.
    assert_eq!(m.history.undo.len(), steps + 1);
    assert_eq!(m.undo_label(), Some("Set 1 value automatically"));
    let row = auto_row(&m).expect("the row");
    assert_eq!(row.text, "1 value set automatically (High confidence)");
    assert_eq!(row.detail.as_deref(), Some("wall.png: cutout"));
    let p = cli_plan(&root, &text(&m));
    assert_eq!(
        cli_job(&p, "g.png").profile.subtype,
        Subtype::LandscapeDiffuse
    );
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

/// AM14: the row's `Choose preset…` opens the header's Preset menu.
#[test]
fn choose_preset_opens_the_preset_menu() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    rgb(&root, "a/rock_n.png", [128, 128, 255]);
    std::fs::write(root.join("sdc.toml"), "preset = \"ghost\"\n").unwrap();
    let mut m = support::opened_as_the_app(root, d.path().join("export"));
    m.toggle_tab(BottomTab::Problems);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert!(h.query_by_label("Presets…").is_none(), "the menu is closed");
    h.get_by_label("Choose preset…").click();
    h.run();
    h.get_by_label("Presets…");
    assert!(h.state().choose_preset.is_none(), "the request was taken");
}

// ---- Note FI4 (owner O1 (b)): an automatic-only state never asks; a user edit does.

const THREE: [&str; 3] = ["a/x_n.png", "a/y_n.png", "a/z_n.png"];

/// Three lone normals: the plan records 3 types, and that step is on top.
fn three_types(dir: &Path) -> (std::path::PathBuf, AppModel) {
    let root = dir.join("textures");
    for k in THREE {
        rgb(&root, k, [128, 128, 255]);
    }
    let m = support::opened_as_the_app(root.clone(), dir.join("export"));
    assert_eq!(m.undo_label(), Some("Set 3 types automatically"));
    assert!(m.is_dirty() && m.only_automatic_unsaved());
    (root, m)
}

fn saved_types(root: &Path) -> usize {
    std::fs::read_to_string(root.join("sdc.toml"))
        .unwrap_or_default()
        .matches("subtype = \"normal\"")
        .count()
}

fn harness(m: AppModel) -> egui_kittest::Harness<'static, AppModel> {
    let h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn an_automatic_only_folder_closes_without_a_dialog() {
    let d = tempfile::tempdir().unwrap();
    let (_, mut m) = three_types(d.path());
    assert!(m.request_close());
    assert_eq!(m.dialog, None);
}

#[test]
fn an_automatic_only_folder_opens_another_without_a_dialog() {
    let d = tempfile::tempdir().unwrap();
    let (_, mut m) = three_types(d.path());
    let other = d.path().join("other");
    rgb(&other, "b/w.png", [90, 60, 30]);
    assert!(m.request_open(other.clone()));
    assert_eq!(m.dialog, None);
    assert_eq!(m.root.as_deref(), Some(other.as_path()));
}

#[test]
fn an_automatic_only_folder_reloads_without_a_dialog() {
    let d = tempfile::tempdir().unwrap();
    let (_, mut m) = three_types(d.path());
    m.request_reload();
    assert_eq!(m.dialog, None);
    assert!(m.wait_idle(WAIT));
    // A new history: the next plan records the same types again.
    assert_eq!(m.undo_label(), Some("Set 3 types automatically"));
    for k in THREE {
        assert_eq!(subtype(&m, k), Some(json!("normal")), "{k}");
    }
}

#[test]
fn save_stays_live_and_writes_the_automatic_types() {
    let d = tempfile::tempdir().unwrap();
    let (root, m) = three_types(d.path());
    assert!(!sdc_gui::ui::header::window_title(&m).contains('●'));
    let mut h = harness(m);
    h.run();
    let save = h.get_by_label("Save");
    assert!(!save.accesskit_node().is_disabled(), "Save is live");
    save.click();
    h.run();
    assert_eq!(saved_types(&root), 3);
    assert!(!h.state().is_dirty());
}

/// Review Focus 5: a user edit on top of the recording ends the no-dialog state; Save writes both.
#[test]
fn a_user_edit_after_the_recording_asks_and_saves_both() {
    let d = tempfile::tempdir().unwrap();
    let (root, mut m) = three_types(d.path());
    m.set_field("a/x_n.png", "format", json!("bc7"));
    assert!(m.wait_idle(WAIT));
    assert!(!m.only_automatic_unsaved());
    assert!(sdc_gui::ui::header::window_title(&m).ends_with(" ●"));
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(sdc_gui::model::Dialog::CloseUnsaved));
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label("Your edits are not saved.");
    h.get_by_label("sdc.toml");
    assert!(
        h.query_by_label_contains("sdc-project.toml").is_none(),
        "only the dirty document is named"
    );
    h.state_mut().dialog = None;
    h.state_mut().save().unwrap();
    let t = std::fs::read_to_string(root.join("sdc.toml")).unwrap();
    assert!(t.contains("format = \"bc7\""), "{t}");
    assert_eq!(saved_types(&root), 3, "{t}");
}

/// Note FI3: the recording a re-encode causes joins the user's step, so the state is a user edit and asks.
#[test]
fn a_reencode_and_its_recording_ask_on_close() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    for k in THREE {
        rgb(&root, k, [128, 128, 255]);
    }
    support::bc1_dds(&root.join("a/v.dds"));
    let mut m = support::opened_as_the_app(root, d.path().join("export"));
    assert!(m.only_automatic_unsaved());
    m.set_reencode(&["a/v.dds".to_string()], true);
    assert!(m.wait_idle(WAIT));
    assert_eq!(m.undo_label(), Some("Re-encode 1 DDS file"));
    assert!(m.prompts_unsaved());
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(sdc_gui::model::Dialog::CloseUnsaved));
}
