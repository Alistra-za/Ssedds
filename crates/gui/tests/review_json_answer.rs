//! Note FC5: Review waits for every overwrite answer (the edited PGPatcher JSON is pinned in review.rs, next to its
//! fixtures), NIF edits are Review rows, and the NIF export is due once after the run Review's Export started.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::meshes::export::GuardChoice;
use sdc_gui::model::meshes::NifReviewRow;
use sdc_gui::model::AppModel;
use std::time::Duration;
use support::*;

const WAIT: Duration = Duration::from_secs(120);

fn row(path: &str) -> NifReviewRow {
    NifReviewRow {
        path: path.into(),
        mod_name: "My Mod".into(),
        blocked: None,
        foreign: None,
        answer: None,
        compaction: None,
    }
}

fn reviewing(dir: &std::path::Path, rows: Vec<NifReviewRow>) -> AppModel {
    let mut m = opened(dir);
    assert!(m.wait_idle(WAIT));
    m.set_nif_review_rows(rows);
    m.export_clicked();
    assert!(m.review.is_some(), "NIF edits open Review");
    m.acknowledge_all_asks();
    m
}

#[test]
fn export_waits_for_a_foreign_nif_answer() {
    let d = tempfile::tempdir().unwrap();
    let sha = [7u8; 32];
    let foreign = NifReviewRow {
        foreign: Some(sha),
        ..row(r"meshes\a\x.nif")
    };
    let mut m = reviewing(d.path(), vec![foreign.clone()]);
    assert_eq!(
        m.review_gate(),
        Err("Answer Keep or Overwrite for x.nif".to_string())
    );
    assert!(!m.export_is_clean());
    m.export_from_review();
    assert!(m.wait_idle(WAIT));
    assert!(
        m.export.summary.is_none(),
        "nothing written before the answer"
    );
    // The answer goes to the NIF side through `app.rs`; its next snapshot carries it.
    m.answer_review_nif(&foreign.path, true, sha);
    assert_eq!(
        m.nif_answer_request,
        Some((foreign.path.clone(), GuardChoice::Overwrite, sha))
    );
    m.set_nif_review_rows(vec![NifReviewRow {
        answer: Some(GuardChoice::Overwrite),
        ..foreign.clone()
    }]);
    assert!(m.review_gate().is_ok(), "{:?}", m.review_gate());
    // A blocked foreign row is skipped, never asked: it does not hold the textures back.
    m.set_nif_review_rows(vec![NifReviewRow {
        blocked: Some("changed on disk since you answered".into()),
        ..foreign
    }]);
    assert!(m.review_gate().is_ok(), "{:?}", m.review_gate());
    // Closing Review drops the NIF answers (GD29).
    m.dismiss_review();
    assert!(m.nif_clear_answers_request);
    assert_eq!(m.nif_answer_request, None);
}

#[test]
fn nif_export_is_due_once_after_the_run_ends() {
    let d = tempfile::tempdir().unwrap();
    let mut m = reviewing(d.path(), vec![row(r"meshes\x.nif")]);
    assert!(!m.take_nif_export_due(), "not before the run");
    m.export_from_review();
    assert!(m.wait_idle(WAIT));
    assert!(m.export.summary.is_some(), "{}", m.status_line);
    assert!(m.take_nif_export_due(), "due after the run");
    assert!(!m.take_nif_export_due(), "once");
    m.nif_export_result(Ok(1));
    assert!(m
        .export
        .notes
        .iter()
        .any(|n| n == "NIF edits written: 1 file"));
    m.nif_export_result(Err(
        r"meshes\y.nif: changed on disk since you answered".into()
    ));
    assert!(m
        .messages
        .entries()
        .any(|e| e.text == r"NIF edits skipped: meshes\y.nif: changed on disk since you answered"));
    // (A cancelled run: `model::tests::a_cancelled_run_makes_no_nif_export_due`.)
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn review_lists_nif_edits_and_a_blocked_one_is_a_warning() {
    let d = tempfile::tempdir().unwrap();
    let rows = vec![
        NifReviewRow {
            compaction: Some(3),
            ..row(r"meshes\a.nif")
        },
        NifReviewRow {
            blocked: Some("conflicts are open".into()),
            ..row(r"meshes\b.nif")
        },
        NifReviewRow {
            foreign: Some([1; 32]),
            ..row(r"meshes\c.nif")
        },
    ];
    let m = reviewing(d.path(), rows);
    let asks = sdc_gui::model::pbr_export::export_asks(&m).len();
    let mut h = harness(m);
    h.run();
    // The blocked one is a warning: listed and counted under Warnings, not in the NIF edits group.
    h.get_by_label("NIF edits (2)");
    h.get_by_label_contains(r"meshes\a.nif · My Mod · 3 unused blocks removed · written to ");
    h.get_by_label("NIF edit skipped: conflicts are open");
    h.get_by_label(&format!("Warnings ({})", asks + 1));
    // A warning never blocks: answering the foreign one is all the gate waits for.
    // The foreign one is a Blockers question with Review's two radios, neither picked.
    h.get_by_label(r"meshes\c.nif was not written by Tessalith");
    h.get_by_label_contains("Blockers (1)");
    h.get_by_label("Overwrite it").click();
    h.run();
    assert_eq!(
        h.state().nif_answer_request,
        Some((r"meshes\c.nif".to_string(), GuardChoice::Overwrite, [1; 32]))
    );
    let rows = h.state().nif_review_rows().to_vec();
    let answered = rows
        .into_iter()
        .map(|r| NifReviewRow {
            answer: r.foreign.map(|_| GuardChoice::Overwrite),
            ..r
        })
        .collect();
    h.state_mut().set_nif_review_rows(answered);
    assert!(
        h.state().review_gate().is_ok(),
        "{:?}",
        h.state().review_gate()
    );
}

#[test]
fn a_review_request_opens_review_on_the_next_poll() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.wait_idle(WAIT));
    m.set_nif_review_rows(vec![row(r"meshes\x.nif")]);
    assert!(m.review.is_none());
    m.open_review_request = true;
    m.poll();
    assert!(m.review.is_some(), "the NIF footer's Review… opens Review");
    assert!(!m.open_review_request, "consumed");
}

/// Note GF2 (ui-rules 3.1): a foreign NIF row's Diff… stays clickable at 1100 px with a long data path: its clipped
/// rect is the whole button (AccessKit bounds are not clipped, so the rect alone proves nothing).
#[test]
fn a_foreign_nif_rows_diff_fits_at_1100_px() {
    let d = tempfile::tempdir().unwrap();
    let path = r"meshes\architecture\whiterun\wrclutter\wrbenchcarvedlong01.nif";
    let m = reviewing(
        d.path(),
        vec![NifReviewRow {
            foreign: Some([1; 32]),
            ..row(path)
        }],
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let r = h
        .get_by_role_and_label(egui::accesskit::Role::Button, &format!("Diff… {path}"))
        .rect();
    let clipped = h.ctx.viewport(|v| {
        v.this_pass
            .widgets
            .layers()
            .flat_map(|(_, ws)| ws)
            .find(|w| w.rect == r)
            .map(|w| w.interact_rect)
    });
    let clipped = clipped.expect("the Diff… widget");
    assert!(
        clipped.width() >= r.width() - 0.5 && clipped.height() >= r.height() - 0.5,
        "Diff… {r:?} is clipped to {clipped:?}"
    );
}
