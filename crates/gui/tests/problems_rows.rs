//! Note FC3: every result the user must act on is a Problems row with its action as the row's button; checks are one
//! count, not amber. The rows whose fixtures live elsewhere are pinned there: GEN-STALE in `generate_from_normal.rs`,
//! CHK-GREEN-GUESS in `checks.rs`, the post-Rebuild-Z row in `bottom_panel.rs`, the cube layout in `cubemap_gui.rs`,
//! the dropped fill in `packing_fills.rs` and the cleared format in `encoding_follows_type.rs`.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use sdc_gui::model::bottom::{self, problems, At, Fix, Level, ProblemsFilter};
use sdc_gui::model::code_text::code_sentence;
use sdc_gui::model::meshes::{NifLevel, NifProblem, NifTab};
use sdc_gui::model::shell::ShellRequest;
use sdc_gui::model::AppModel;
use sdc_gui::ui::status::Status;
use std::sync::Arc;
use std::time::Duration;
use support::*;

fn harness(mut m: AppModel) -> Harness<'static, AppModel> {
    m.open_problems(None);
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn idle(h: &mut Harness<'static, AppModel>) {
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

fn row_of(m: &AppModel, code: Code) -> bottom::Problem {
    problems(m)
        .iter()
        .find(|p| p.code == Some(code))
        .cloned()
        .unwrap_or_else(|| panic!("no {} row: {:?}", code.as_str(), problems(m)))
}

/// Y4: an entry the plan could not read is a row; `Open folder` opens the folder that holds it.
#[test]
fn an_unreadable_entry_is_a_row_with_open_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    let folder = d.path().join("textures").join("landscape");
    let msg = format!(
        "skipped: {}: Access is denied. (os error 5)",
        folder.join("locked").display()
    );
    m.unreadable = vec![Warning::new(Code::InUnreadable, msg)];
    m.invalidate_views();
    let row = row_of(&m, Code::InUnreadable);
    assert_eq!((row.level, &row.at), (Level::Warning, &At::Project));
    assert_eq!(row.fix, Some(Fix::OpenFolder(folder.clone())));
    let mut h = harness(m);
    h.run();
    h.get_by_label(code_sentence(Code::InUnreadable));
    assert!(h.state().shell.is_empty());
    h.get_by_label("Open folder").click();
    h.run();
    assert_eq!(h.state().shell, vec![ShellRequest::Open(folder)]);
}

/// CN67: DERIVE-NO-ENVMAP and DERIVE-NIF-SUGGEST are engine Info; in the GUI they are Warning rows with `Open report`.
/// Other Info results of the last export stay off the list.
#[test]
fn derive_results_are_rows_that_open_the_report() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    let report = d.path().join("export").join("x conversion-report.md");
    m.also_report = Some(report.clone());
    m.export.rows[0].warnings.extend([
        Warning::new(
            Code::DeriveNoEnvmap,
            "set x: metal found, but no scanned mesh uses it",
        ),
        Warning::new(Code::DeriveNotConverted, "not converted"),
    ]);
    m.export.rows[1].warnings.push(Warning::new(
        Code::DeriveNifSuggest,
        "suggested NIF values: glossiness 30.0, specular strength 1.0",
    ));
    m.invalidate_views();
    for c in [Code::DeriveNoEnvmap, Code::DeriveNifSuggest] {
        let row = row_of(&m, c);
        assert_eq!(row.level, Level::Warning, "{c:?}");
        assert!(matches!(row.at, At::File(_)));
        assert_eq!(row.fix, Some(Fix::OpenReport(report.clone())));
        assert!(!code_sentence(c).is_empty());
    }
    assert!(!problems(&m)
        .iter()
        .any(|p| p.code == Some(Code::DeriveNotConverted)));
    let mut h = harness(m);
    h.run();
    h.get_by_label(code_sentence(Code::DeriveNifSuggest));
    h.query_all_by_label("Open report").next().unwrap().click();
    h.run();
    assert_eq!(h.state().shell, vec![ShellRequest::Open(report)]);
}

/// The fixture with only the plastic check on and one plain warning row: a Warning row and a Check row.
fn with_a_check(d: &std::path::Path) -> AppModel {
    let mut m = opened(d);
    decide_all(&mut m);
    m.set_checks(sdc_gui::settings::ChecksSettings {
        range: false,
        plastic: true,
        low_detail: false,
        green: false,
        parallax: false,
        uv: false,
        drop_top_mip: false,
    });
    m.poll();
    m.invalidate_views();
    m
}

/// CL5/CL18 amended: a check row is drawn weak, not amber, and is no ready signal: the set dot and the project's
/// clean state are what they are with the check off.
#[test]
fn checks_are_not_amber_and_not_blocking() {
    let d = tempfile::tempdir().unwrap();
    let m = with_a_check(d.path());
    let checks: Vec<_> = problems(&m)
        .iter()
        .filter(|p| p.level == Level::Check)
        .cloned()
        .collect();
    assert_eq!(checks.len(), 1, "{:?}", problems(&m));
    assert!(!bottom::counts_as_ready_problem(&checks[0]));
    let status = sdc_gui::ui::problems::status_of(Level::Check);
    assert_eq!(status, Status::Check);
    assert!(status.weak());
    let At::Texture(k) = &checks[0].at else {
        panic!("{:?}", checks[0].at)
    };
    let views = sdc_gui::model::sets::views(&m);
    let v = views
        .iter()
        .find(|v| v.members.contains(k))
        .expect("its set");
    // This fixture's set also has a plain warning, so the ready signals are compared with the check off; the green
    // dot and a clean project with only a check row are pinned in `checks.rs` (`soft_check_rows_are_listed_but_leave_
    // ready_alone`).
    let ready = bottom::set_ready(&m, v);
    let clean = m.export_is_clean();
    let mut off = with_a_check(tempfile::tempdir().unwrap().path());
    off.set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..off.settings.checks
    });
    off.poll();
    off.invalidate_views();
    assert!(problems(&off).iter().all(|p| p.level != Level::Check));
    let off_views = sdc_gui::model::sets::views(&off);
    let off_v = off_views
        .iter()
        .find(|w| w.members.contains(k))
        .expect("its set, check off");
    assert_eq!(
        ready,
        bottom::set_ready(&off, off_v),
        "a check changes no set dot"
    );
    assert_eq!(
        clean,
        off.export_is_clean(),
        "a check changes nothing of export_is_clean"
    );
    let mut h = harness(m);
    h.run();
    // The row's dot is a "check" dot, never a "warning" one.
    let sentence = checks[0].shown().to_string();
    let row = h.get_by_label(&sentence).rect();
    assert!(h
        .query_all_by_label("check")
        .any(|n| (n.rect().center().y - row.center().y).abs() < 8.0));
    assert!(!h
        .query_all_by_label("warning")
        .any(|n| (n.rect().center().y - row.center().y).abs() < 8.0));
}

/// CL18 amended: one count (the tab's), and checks are a `Checks` pill that toggles the filter (rule 6.5).
#[test]
fn problems_shows_one_count() {
    let d = tempfile::tempdir().unwrap();
    let m = with_a_check(d.path());
    let n = problems(&m).len();
    let mut h = harness(m);
    h.run();
    h.get_by_label(&format!("Problems ({n})"));
    assert!(
        h.query_all_by_label_contains("Checks ").next().is_none(),
        "no second count"
    );
    h.get_by_label(sdc_gui::ui::problems::CHECKS_PILL).click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Checks));
    h.get_by_label(sdc_gui::ui::problems::CHECKS_PILL).click();
    h.run();
    assert_eq!(h.state().bottom.filter, None);
}

fn nif(level: NifLevel, path: &str, text: &str, tab: NifTab) -> NifProblem {
    NifProblem {
        level,
        path: path.into(),
        text: text.into(),
        tab,
        field: None,
    }
}

/// An Info row (here a NIF one) never takes "ready" away: the flagged counts do not move.
#[test]
fn info_rows_never_count_against_ready() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    let before = bottom::flagged(&m).ready_rows;
    let checks_before = bottom::flagged(&m).check_rows;
    let clean = m.export_is_clean();
    m.set_nif_problems(Arc::new(vec![nif(
        NifLevel::Info,
        "meshes/a.nif",
        "an info line",
        NifTab::Extra,
    )]));
    let rows: Vec<_> = problems(&m)
        .iter()
        .filter(|p| p.level == Level::Info)
        .cloned()
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(!bottom::counts_as_ready_problem(&rows[0]));
    assert_eq!(bottom::flagged(&m).ready_rows, before);
    assert_eq!(
        bottom::flagged(&m).check_rows,
        checks_before,
        "an Info row is not a check"
    );
    assert_eq!(m.export_is_clean(), clean);
}

/// GD1 interface: NIF Blocks and Warning map to rows at `At::Nif` whose fix asks for the NIF tab; the same snapshot
/// again does not rebuild Problems.
#[test]
fn nif_problems_map_to_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    let snap = Arc::new(vec![
        nif(
            NifLevel::Blocks,
            "meshes/a.nif",
            "a blocker",
            NifTab::Controllers,
        ),
        nif(
            NifLevel::Warning,
            "meshes/b.nif",
            "slot 5 is empty",
            NifTab::Shader,
        ),
    ]);
    m.set_nif_problems(snap.clone());
    let rows: Vec<_> = problems(&m)
        .iter()
        .filter(|p| matches!(p.at, At::Nif(_)))
        .cloned()
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].level, Level::Blocks);
    assert_eq!(rows[0].at, At::Nif("meshes/a.nif".into()));
    assert_eq!(
        rows[0].fix,
        Some(Fix::ShowNifTab(
            "meshes/a.nif".into(),
            NifTab::Controllers,
            None
        ))
    );
    assert_eq!(rows[1].level, Level::Warning);
    assert_eq!(rows[1].text, "slot 5 is empty");
    let builds = bottom::problems_builds(&m);
    m.set_nif_problems(snap);
    problems(&m);
    assert_eq!(bottom::problems_builds(&m), builds, "same Arc, no rebuild");
    let mut h = harness(m);
    h.run();
    h.get_by_label("Open the Shader tab").click();
    h.run();
    assert_eq!(
        h.state().nif_tab_request,
        Some(("meshes/b.nif".into(), NifTab::Shader, None))
    );
}

fn rock_id(m: &AppModel) -> (String, String) {
    let v = sdc_gui::model::sets::views(m);
    let v = v.iter().find(|v| v.key.ends_with("rock01")).unwrap();
    (v.id.clone(), v.key.clone())
}

/// P26: two features that can't both be on block; the row turns the second one off.
#[test]
fn a_feature_clash_offers_turn_it_off() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let (id, _) = rock_id(&m);
    m.set_json_value(&id, "hair", Some(serde_json::json!(true)));
    m.set_json_value(&id, "glint_on", Some(serde_json::json!(true)));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let row = problems(&m)
        .iter()
        .find(|p| p.text.contains("can't both be on"))
        .cloned()
        .expect("the clash");
    assert_eq!(row.level, Level::Blocks);
    assert!(matches!(&row.fix, Some(Fix::TurnFeatureOff(i, _)) if *i == id));
    let mut h = harness(m);
    h.run();
    h.get_by_label("Turn glint off").click();
    idle(&mut h);
    assert!(!problems(h.state())
        .iter()
        .any(|p| p.text.contains("can't both be on")));
}

/// P13: a path that blocks goes to its set and focuses the field that holds it.
#[test]
fn a_path_blocker_offers_fix_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let (id, key) = rock_id(&m);
    m.set_json_value(&id, "rename", Some(serde_json::json!("sub\\name")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let row = problems(&m)
        .iter()
        .find(|p| p.text.contains("rename:"))
        .cloned()
        .expect("the rename blocker");
    assert_eq!(row.fix, Some(Fix::FixPath(key.clone(), "rename".into())));
    let mut h = harness(m);
    h.run();
    h.get_by_label("Fix path").click();
    for _ in 0..4 {
        h.run();
    }
    assert!(
        matches!(&h.state().selection, sdc_gui::model::Selection::Set(k) if *k == key),
        "the set is selected"
    );
    let focused = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().is_focused())
        .expect("a focused text field");
    assert_eq!(
        focused.accesskit_node().value().as_deref(),
        Some("sub\\name")
    );
}
