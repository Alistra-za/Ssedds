//! Note FB1 (review C2, amendment B): a project never inherits another project's export folder from `gui.toml`; the
//! header's `Export to:` chip says `not chosen` until one is, and its Export button reads `Review {n} files` until
//! Review would write without skipping anything (Note FI5 amended FB1's "chip only when chosen").
mod support;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::picker::{PickerBackend, PickerSpec};
use sdc_gui::model::AppModel;
use sdc_gui::settings::{GuiSettings, MeshesSettings};
use sdc_gui::ui::meshes::UndoTarget;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use support::*;

const NO_FOLDER: &str = "No export folder chosen";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// The mod fixture opened with `gui.toml` holding another mod's export folder and no per-project choice.
fn fresh(dir: &std::path::Path) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("other-mod")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(mod_folder(dir));
    idle(&mut m);
    m
}

#[test]
fn a_fresh_project_does_not_inherit_the_gui_toml_export_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = fresh(d.path());
    assert_eq!(m.export_folder_chosen(), None);
    assert_eq!(m.export_root(), m.root.clone().unwrap().join("sdc-out"));
    assert_eq!(m.review_gate(), Err(NO_FOLDER.to_string()));
}

#[test]
fn picking_a_folder_clears_the_blocker() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    let out = d.path().join("out");
    m.set_export_folder(out.clone());
    idle(&mut m);
    assert_eq!(m.export_folder_chosen(), Some(out.clone()));
    assert_eq!(m.export_root(), out);
    assert_ne!(m.review_gate(), Err(NO_FOLDER.to_string()));
}

/// Review Focus 1: with no usable project file the pick lives in `gui.toml` (Note BV6), so it counts as chosen, or the
/// user could never export.
#[test]
fn without_a_usable_project_file_gui_toml_still_counts() {
    let d = tempfile::tempdir().unwrap();
    let other = d.path().join("other-mod");
    // No folder open at all.
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(other.clone()),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert_eq!(m.export_folder_chosen(), Some(other.clone()));

    // A project file that does not parse: read-only, so the folder is picked into gui.toml.
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc-project.toml"), "target = [\n").unwrap();
    let mut m = AppModel::new(
        GuiSettings {
            export_folder: Some(other.clone()),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    m.open_folder(root);
    idle(&mut m);
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    assert_eq!(m.export_folder_chosen(), Some(other));
    let out = d.path().join("out");
    m.set_export_folder(out.clone());
    assert_eq!(m.settings.export_folder, Some(out.clone()));
    assert_eq!(m.export_folder_chosen(), Some(out));
}

#[test]
fn header_export_reads_review_until_clean() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    m.set_export_folder(d.path().join("out"));
    idle(&mut m);
    // The fixture has textures that are undecided.
    assert!(m.undecided() > 0);
    assert!(
        m.header_export_text().starts_with("Review "),
        "{}",
        m.header_export_text()
    );
    assert!(!m.export_is_clean());
    decide_all(&mut m);
    // K3: the fixture's flat `_normal` raises CHK-PLASTIC, which is not about this test.
    m.set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..Default::default()
    });
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    let n = m.export_files();
    assert_eq!(
        m.header_export_text(),
        format!("Export {n} file{}", if n == 1 { "" } else { "s" })
    );
    assert!(m.export_is_clean());
    // Note FC14 (final review, minor): a blocked NIF edit is skipped, never written, so it keeps the project clean;
    // one Review's Export would write does not.
    let nif = |blocked: Option<&str>| sdc_gui::model::meshes::NifReviewRow {
        path: r"meshes\x.nif".into(),
        mod_name: "My Mod".into(),
        blocked: blocked.map(String::from),
        foreign: None,
        answer: None,
        compaction: None,
    };
    m.set_nif_review_rows(vec![nif(Some("its NIF changed on disk"))]);
    assert!(m.export_is_clean());
    m.set_nif_review_rows(vec![nif(None)]);
    assert!(!m.export_is_clean());
}

fn nif_row(blocked: Option<&str>) -> sdc_gui::model::meshes::NifReviewRow {
    sdc_gui::model::meshes::NifReviewRow {
        path: r"meshes\x.nif".into(),
        mod_name: "My Mod".into(),
        blocked: blocked.map(String::from),
        foreign: None,
        answer: None,
        compaction: None,
    }
}

/// A decided project with a folder chosen, so only the NIF rows decide the header's verb.
fn decided_with_folder(dir: &std::path::Path) -> AppModel {
    let mut m = fresh(dir);
    m.set_export_folder(dir.join("out"));
    idle(&mut m);
    decide_all(&mut m);
    m.set_checks(sdc_gui::settings::ChecksSettings {
        plastic: false,
        ..Default::default()
    });
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    m
}

fn files(m: &AppModel) -> String {
    let n = m.export_files();
    format!("{n} file{}", if n == 1 { "" } else { "s" })
}

/// Note FI7 (4a): a pending NIF edit makes the header `Review …`, which writes nothing, so it counts the edit.
#[test]
fn the_header_counts_pending_nif_edits() {
    let d = tempfile::tempdir().unwrap();
    let mut m = decided_with_folder(d.path());
    m.set_nif_review_rows(vec![nif_row(None)]);
    let f = files(&m);
    assert_eq!(m.header_export_text(), format!("Review {f} + 1 NIF edit"));
    assert_eq!(m.review_button_text(), format!("Export {f} + 1 NIF edit"));
}

/// Review Focus 6: Review's button names only the NIF edits its Export writes; a blocked one joins the skip list.
#[test]
fn the_export_button_does_not_count_a_blocked_nif_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = decided_with_folder(d.path());
    m.set_nif_review_rows(vec![
        nif_row(Some("Resolve the conflicts first.")),
        nif_row(None),
    ]);
    let f = files(&m);
    assert_eq!(m.header_export_text(), format!("Review {f} + 2 NIF edits"));
    assert_eq!(
        m.review_button_text(),
        format!("Export {f} + 1 NIF edit (skip 1 NIF edit)")
    );
    assert!(m.review_gate().is_ok(), "the button stays enabled");
    // Only blocked rows: nothing to write, only a skip.
    m.set_nif_review_rows(vec![nif_row(Some("x"))]);
    assert_eq!(
        m.review_button_text(),
        format!("Export {f} (skip 1 NIF edit)")
    );
}

/// Note FI7 (4a, round 2): a clean header click writes the files at once, so it names no NIF edit as written.
#[test]
fn a_clean_header_export_names_no_blocked_nif_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = decided_with_folder(d.path());
    m.set_nif_review_rows(vec![nif_row(Some("its NIF changed on disk"))]);
    assert!(m.export_is_clean());
    let f = files(&m);
    assert_eq!(
        m.header_export_text(),
        format!("Export {f} (skip 1 NIF edit)")
    );
    m.export_clicked();
    idle(&mut m);
    assert!(m.review.is_none(), "{}", m.status_line);
    assert!(m.export.summary.is_some(), "{}", m.status_line);
    assert!(!m.take_nif_export_due(), "no NIF edit was written");
}

#[test]
fn without_a_chosen_folder_the_text_is_review_even_when_everything_is_decided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    decide_all(&mut m);
    assert_eq!(m.undecided(), 0);
    assert!(m.header_export_text().starts_with("Review "));
    assert!(!m.export_is_clean());
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Note FI5 (O2; supersedes FB1's "chip only when chosen"): a project is open and none is chosen, so the header says so.
#[test]
fn the_header_says_the_folder_is_not_chosen() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(fresh(d.path()));
    h.run();
    h.get_by_role_and_label(Role::Button, "Export to: not chosen");
    assert_eq!(h.query_all_by_label("More export options").count(), 0);
    // Once chosen it names the folder.
    let m = h.state_mut();
    m.set_export_folder(d.path().join("out"));
    idle(m);
    h.run();
    assert_eq!(h.query_all_by_label_contains("Export to:").count(), 1);
    assert_eq!(h.query_all_by_label("Export to: not chosen").count(), 0);
}

/// Note FI5 (A4, Important 7): with no project the `gui.toml` folder is not shown (it vanishes when one opens), so the
/// menu is `More`; a project with no choice of its own then reads `Export to: not chosen`.
#[test]
fn the_start_screen_shows_more_even_with_a_gui_toml_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(d.path().join("out")),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert!(m.export_folder_chosen().is_some());
    let mut h = harness(m);
    h.run();
    h.get_by_role_and_label(Role::Button, "More export options");
    assert_eq!(h.query_all_by_label_contains("Export to:").count(), 0);
    let root = mod_folder(d.path());
    h.state_mut().open_folder(root);
    idle(h.state_mut());
    h.run();
    h.get_by_role_and_label(Role::Button, "Export to: not chosen");
    assert_eq!(h.query_all_by_label("More export options").count(), 0);
}

type UndoH = Harness<'static, (AppModel, MeshesModel, Option<String>)>;

/// The header with the Undo target stubbed: `None` is the Textures history (`Undo`), `Some(step)` a NIF edit whose next
/// undo step is `step` (`Undo NIF edit: {step}`).
fn undo_harness(dir: &std::path::Path) -> UndoH {
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    meshes.procs = sdc_gui::model::meshes::no_procs;
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, (m, me, step): &mut (AppModel, MeshesModel, Option<String>)| {
                sdc_gui::ui::header::set_meshes_view(ui.ctx(), step.is_some());
                let target = step.as_ref().map(|s| UndoTarget {
                    can_undo: true,
                    can_redo: false,
                    undo_label: Some(s.clone()),
                    redo_label: None,
                });
                ui.ctx()
                    .data_mut(|d| d.insert_temp(eframe::egui::Id::new("nif-undo-target"), target));
                sdc_gui::ui::header::draw(ui, m, Some(&mut *me));
            },
            (fresh(dir), meshes, None),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Note FI5 (A19): the Undo slot has a fixed width, so a long NIF step label does not move `MO2…` and the rest of the row.
#[test]
fn undo_text_does_not_move_the_header() {
    let d = tempfile::tempdir().unwrap();
    let mut h = undo_harness(d.path());
    h.run_steps(4);
    let (mo2, undo) = (h.get_by_label("MO2…").rect(), h.get_by_label("Undo").rect());
    let long = "Add alpha to every shape";
    h.state_mut().2 = Some(long.to_string());
    h.run_steps(4);
    let b = h.get_by_label("Undo (NIF edit history)");
    assert_eq!(b.rect(), undo, "the slot");
    assert_eq!(h.get_by_label("MO2…").rect().min.x, mo2.min.x);
    // The text is cut in the slot; the whole step is on hover.
    b.hover();
    h.run();
    h.get_by_label_contains(&format!("Undo NIF edit: {long}"));
}

/// The picker backend that records where the dialog was asked to start.
struct Recorder(std::sync::Mutex<Vec<Option<PathBuf>>>, PathBuf);

impl PickerBackend for Recorder {
    fn pick(&self, spec: &PickerSpec) -> Option<PathBuf> {
        self.0.lock().unwrap().push(spec.start.clone());
        Some(self.1.clone())
    }
}

#[test]
fn review_lists_the_blocker_and_its_choose_button_picks_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    let out = d.path().join("picked");
    let rec = Arc::new(Recorder(Default::default(), out.clone()));
    m.picker.set_backend(rec.clone());
    m.open_review();
    let mut h = harness(m);
    h.run();
    // The footer repeats the gate's reason; the Blockers column has its own row.
    assert!(h.query_all_by_label(NO_FOLDER).count() >= 1);
    // The missing folder counts in the heading, because the gate enforces it (Note FB4).
    h.get_by_label("Blockers (1)");
    // Rule 5.4: the button draws its own name, and Review has no bare `Choose…`.
    h.get_by_role_and_label(Role::Button, "Choose the export folder…");
    assert_eq!(
        h.query_all_by_role(Role::Button)
            .filter(|n| n.accesskit_node().label().as_deref() == Some("Choose…"))
            .count(),
        0
    );
    h.get_by_label("Choose the export folder…").click();
    h.run();
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while h.state().picker.busy() {
        assert!(std::time::Instant::now() < end, "the dialog never closed");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.state_mut().poll());
    idle(h.state_mut());
    assert_eq!(h.state().export_folder_chosen(), Some(out));
    // The dialog opened where gui.toml last pointed.
    assert_eq!(
        rec.0.lock().unwrap().clone(),
        vec![Some(d.path().join("other-mod"))]
    );
    h.run();
    assert_eq!(h.query_all_by_label(NO_FOLDER).count(), 0);
    h.get_by_label("Blockers (0)");
}

/// Note FI5 (O4): the stacked layout draws the export folder row too, under `Blockers (1)`. A guard: it held before.
#[test]
fn the_export_folder_row_shows_in_the_stacked_review() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    m.open_review();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Blockers (1)");
    assert!(h.query_all_by_label(NO_FOLDER).count() >= 1);
    h.get_by_role_and_label(Role::Button, "Choose the export folder…");
}

/// Note FI5 (A5, O4): with no folder chosen nothing in the window shows a `sdc-out` path. The Also column is headed
/// `Also folders go in`, its location says the export folder is not chosen yet, and JSON lines are relative to it.
#[test]
fn no_sdc_out_path_is_shown_before_a_folder_is_chosen() {
    use engine::config::{GreenConvention, Subtype};
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::PbrNormal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    idle(&mut m);
    m.set_also_tick(engine::presets::AlsoTarget::Vanilla, true);
    idle(&mut m);
    assert!(m.also_on() && m.export_folder_chosen().is_none());
    m.open_review();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1400.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    // Every label and value in the window, then the hover of the two Choose buttons.
    let all = |h: &Harness<'static, AppModel>| -> Vec<String> {
        h.query_all(egui_kittest::kittest::by().predicate(|_| true))
            .flat_map(|n| {
                let a = n.accesskit_node();
                [a.label(), a.value()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    assert!(
        !all(&h).iter().any(|t| t.contains("sdc-out")),
        "{:?}",
        all(&h)
    );
    h.get_by_label("Also folders go in");
    assert_eq!(h.query_all_by_label_contains("Export location").count(), 0);
    h.get_by_label_contains("Also folders go in: the export folder (not chosen yet)");
    h.get_by_role_and_label(Role::Button, "Choose where the Also folders go…")
        .hover();
    h.run();
    assert!(!all(&h).iter().any(|t| t.contains("sdc-out")));
    h.get_by_role_and_label(Role::Button, "Choose the export folder…")
        .hover();
    h.run();
    assert!(!all(&h).iter().any(|t| t.contains("sdc-out")));
    // The JSON line, if it is drawn (the footer scrolls): it is relative to the export folder.
    let json: Vec<String> = all(&h)
        .into_iter()
        .filter(|t| t.contains(".json"))
        .collect();
    assert!(
        json.iter().all(|t| t.starts_with("‹export folder›\\")),
        "{json:?}"
    );
    assert!(!json.is_empty(), "the JSON line is drawn");
}

/// A clean project with no folder chosen does not write into `sdc-out`: the click opens Review instead.
#[test]
fn exporting_without_a_chosen_folder_opens_review_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fresh(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.review.is_some());
    assert!(!m.root.clone().unwrap().join("sdc-out").exists());
}

/// Note FB1 (NIF team GD5): NIF edits need a chosen folder too; they never go to `<root>\sdc-out`.
#[test]
fn nif_edits_need_a_chosen_folder() {
    let d = tempfile::tempdir().unwrap();
    // gui.toml names a folder but no project is open.
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(d.path().join("other-mod")),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert_eq!(m.nif_export_root(), Some(d.path().join("other-mod")));
    // A fresh project does not inherit it, so there is no root (not `<root>\sdc-out`).
    let mut m = fresh(d.path());
    assert_eq!(m.nif_export_root(), None);
    let x = d.path().join("x");
    m.set_export_folder(x.clone());
    idle(&mut m);
    assert_eq!(m.nif_export_root(), Some(x));
}

/// An empty project has nothing to write, so its Export is never accent "ready" (A12 withheld it too).
#[test]
fn an_empty_project_is_not_clean() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    std::fs::create_dir_all(&root).unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.open_folder(root);
    idle(&mut m);
    m.set_export_folder(d.path().join("out"));
    idle(&mut m);
    assert_eq!(m.export_files(), 0);
    assert!(!m.export_is_clean());
    assert_eq!(m.header_export_text(), "Review 0 files");
}

/// Final review M5: with no folder open the button is plain "Export", not "Review 0 files".
#[test]
fn no_folder_open_reads_plain_export() {
    let m = AppModel::new(GuiSettings::default(), None, session());
    assert_eq!(m.header_export_text(), "Export");
}

/// Note FI18 (3; HANDOFF 2026-11-01 owed item 3): with no folder open the header counts pending NIF edits as the
/// project case does (Note FI7): `Review k NIF edit(s)`, blocked ones included; none pending keeps plain "Export".
#[test]
fn no_folder_open_counts_pending_nif_edits() {
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.set_nif_review_rows(vec![]);
    assert_eq!(m.header_export_text(), "Export");
    m.set_nif_review_rows(vec![nif_row(None)]);
    assert_eq!(m.header_export_text(), "Review 1 NIF edit");
    m.set_nif_review_rows(vec![
        nif_row(None),
        nif_row(Some("its NIF changed on disk")),
    ]);
    assert_eq!(m.header_export_text(), "Review 2 NIF edits");
}

/// UI-D final review Minor 5: with no folder open, the disabled header `Review 1 NIF edit` says on hover where Review
/// opens from.
#[test]
fn the_disabled_no_project_review_button_says_where_review_opens() {
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.set_nif_review_rows(vec![nif_row(None)]);
    let mut h = harness(m);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    h.get_by_role_and_label(Role::Button, "Review 1 NIF edit")
        .hover();
    h.run();
    h.get_by_label("Open Review from the NIF edit panel (Review…)");
}

/// Note GH8: a fresh start (no project, no folder, no edit) still says "Open a folder first", not the folder reason.
#[test]
fn fresh_start_with_no_folder_says_open_a_folder_first() {
    let m = AppModel::new(GuiSettings::default(), None, session());
    assert_eq!(m.review_gate(), Err("Open a folder first".to_string()));
}

/// Note GH8: with no project and a folder chosen, Review has nothing to write until a NIF edit is pending.
#[test]
fn meshes_only_with_nothing_to_write_says_open_a_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = AppModel::new(
        GuiSettings {
            export_folder: Some(d.path().join("out")),
            ..GuiSettings::default()
        },
        None,
        session(),
    );
    assert_eq!(m.review_gate(), Err("Open a folder first".to_string()));
    assert_eq!(m.review_button_text(), "Export 0 NIF edits");
}
