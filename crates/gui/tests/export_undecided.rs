//! Task 3: the undecided textures are listed and resolved before an export (owner). Layout v2 Task 1.3: the list is
//! Review's (a bottom-panel tab), no longer a modal; `rows_decided_in_the_dialog_go_back_through_the_export_blocks`
//! and `a_texture_without_a_sub_type_suggestion_stays_listed_and_undecided` moved to review.rs.
mod support;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use sdc_gui::model::*;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const ODD: &str = "landscape/odd.png";
const NORMAL: &str = "landscape/rock01_normal.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "model did not settle"
    );
}

/// Review's undecided list (it must be open).
fn listed(m: &AppModel) -> Vec<String> {
    assert!(m.review.is_some(), "expected Review, dialog {:?}", m.dialog);
    m.undecided_keys()
}

/// Every non-sub-type question answered, so only the sub-types are still undecided.
fn answer_all_but_subtypes(m: &mut AppModel) {
    m.decide(LEAF, Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(ODD, Answer::Acknowledge(Code::DimNpot));
    m.decide(NORMAL, Answer::Green(GreenConvention::Directx));
    idle(m);
}

#[test]
fn review_lists_every_undecided_texture_with_its_suggestion() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    let keys = listed(&m);
    assert_eq!(keys.len(), m.undecided());
    assert_eq!(keys.len(), 4);
    for k in &keys {
        assert!(matches!(m.subtype_question(k), Some(Some(_))), "{k}");
    }
}

#[test]
fn accept_all_decides_the_suggestions_and_the_export_proceeds_on_the_settled_plan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    m.export_clicked();
    assert_eq!(listed(&m).len(), m.undecided());
    assert!(m.undecided() >= 2);
    m.accept_all_suggestions();
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    // R2: Review never exports by itself; its Export does.
    assert!(m.export.summary.is_none());
    m.export_from_review();
    idle(&mut m);
    let s = m.export.summary.clone().expect("the export ran");
    assert!(s.converted >= 1);
    // The export used the fresh plan: the landscape diffuse was decided and exported.
    assert!(d.path().join("export/landscape/rock01.dds").exists());
    assert_eq!(
        m.item(ROCK)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype),
        Some(Subtype::LandscapeDiffuse)
    );
}

#[test]
fn deciding_one_row_removes_only_that_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    let before = listed(&m);
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    idle(&mut m);
    let after = listed(&m);
    assert_eq!(after.len(), before.len() - 1);
    assert!(!after.contains(&ROCK.to_string()));
    idle(&mut m);
    assert_eq!(
        m.item(ROCK)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype),
        Some(Subtype::Diffuse)
    );
}

#[test]
fn review_export_still_skips_what_is_undecided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    let n = listed(&m).len();
    m.export_from_review();
    assert_eq!(m.dialog, None);
    idle(&mut m);
    assert_eq!(m.undecided(), n);
    assert!(m.export.summary.is_some());
    assert!(!d.path().join("export/landscape/leaf.dds").exists());
}

#[test]
fn review_export_after_a_row_waits_for_the_plan_of_that_decision() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    // The decision's plan is in flight: Export does not export on the stale plan.
    m.export_from_review();
    assert!(m.export.running.is_none());
    idle(&mut m);
    assert!(m.export.summary.is_some());
    assert!(d.path().join("export/landscape/rock01.dds").exists());
}

#[test]
fn one_undo_reverts_accept_all() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    let steps = m.history.undo.len();
    m.export_clicked();
    let n = listed(&m).len();
    assert!(n >= 2);
    m.accept_all_suggestions();
    assert_eq!(m.history.undo.len(), steps + 1, "Accept all is one step");
    idle(&mut m);
    m.undo();
    idle(&mut m);
    assert_eq!(m.undecided(), n);
    assert_eq!(m.history.undo.len(), steps);
}

#[test]
fn a_pending_continuation_does_not_outlive_the_export_the_user_clicked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    // Review's Export waits for the plan in flight; the header's Export is clicked meanwhile and takes over.
    m.export_from_review();
    m.export_clicked();
    idle(&mut m);
    // The clicked export opened Review again (other textures are undecided); its Export exports once.
    assert!(m.export.summary.is_none());
    listed(&m);
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_some());
    // A later, unrelated plan starts nothing.
    m.export.summary = None;
    m.set_field(ROCK, "mipmaps.count", serde_json::json!(3));
    idle(&mut m);
    assert!(m.export.summary.is_none());
    assert!(m.export.running.is_none());
    assert_eq!(m.dialog, None);
}

/// Audit 2026-10-14 finding 2: a folder opened while Review is up closes it and starts nothing in the new folder.
#[test]
fn opening_another_folder_closes_review_and_exports_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    listed(&m);
    let d2 = tempfile::tempdir().unwrap();
    let root2 = mod_folder(d2.path());
    assert!(m.request_open(root2.clone()));
    idle(&mut m);
    assert_eq!(m.review, None);
    assert_eq!(m.dialog, None);
    assert!(m.export.summary.is_none(), "no export ran");
    assert!(!root2.join("sdc.toml").exists(), "nothing was written");
}

#[test]
fn rows_waiting_on_alpha_are_answered_inline_and_leave_when_decided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.decide_undecided_row(LEAF, Subtype::Diffuse);
    idle(&mut m);
    // Its sub-type is decided, but it still waits for its alpha meaning: the row stays.
    assert!(listed(&m).contains(&LEAF.to_string()));
    m.answer_undecided_row(LEAF, Answer::Alpha(AlphaPolicy::Cutout));
    idle(&mut m);
    assert!(!listed(&m).contains(&LEAF.to_string()));
}

/// Audit 11 (Note Y8): more picks than the history keeps are still one step, undone at once.
#[test]
fn accept_all_over_the_history_limit_is_one_undo_step() {
    use sdc_gui::model::history::History;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    let rock = m.root.clone().unwrap().join(ROCK);
    for i in 0..History::LIMIT + 5 {
        std::fs::copy(&rock, rock.with_file_name(format!("rock{i:03}.png"))).unwrap();
    }
    m.replan();
    idle(&mut m);
    let undecided = m.undecided();
    assert!(undecided > History::LIMIT, "{undecided}");
    m.open_review();
    let steps = m.history.undo.len();
    m.accept_all_suggestions();
    assert_eq!(m.history.undo.len(), steps + 1, "one step");
    idle(&mut m);
    assert_eq!(m.undecided(), 0);
    m.undo();
    idle(&mut m);
    assert_eq!(m.undecided(), undecided, "one undo restores every pick");
}
