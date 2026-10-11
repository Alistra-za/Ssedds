//! Task 3: the export dialog lists the undecided textures and resolves them (owner).
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

fn listed(m: &AppModel) -> Vec<String> {
    match &m.dialog {
        Some(Dialog::ProceedAnyway(k)) => k.clone(),
        other => panic!("expected the undecided dialog, got {other:?}"),
    }
}

/// Every non-sub-type question answered, so only the sub-types are still undecided.
fn answer_all_but_subtypes(m: &mut AppModel) {
    m.decide(LEAF, Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(ODD, Answer::Acknowledge(Code::DimNpot));
    m.decide(NORMAL, Answer::Green(GreenConvention::Directx));
    idle(m);
}

#[test]
fn the_dialog_lists_every_undecided_texture_with_its_suggestion() {
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
fn a_texture_without_a_sub_type_suggestion_stays_listed_and_undecided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // leaf's sub-type is decided; it now waits for its alpha meaning, which Accept all never answers.
    m.decide(LEAF, Answer::Subtype(Subtype::Diffuse));
    idle(&mut m);
    assert_eq!(m.subtype_question(LEAF), None);
    assert!(m.undecided_keys().contains(&LEAF.to_string()));
    m.export_clicked();
    m.accept_all_suggestions();
    let left = listed(&m);
    assert!(left.contains(&LEAF.to_string()), "{left:?}");
    assert!(m.export.running.is_none());
    idle(&mut m);
    assert!(m.undecided_keys().contains(&LEAF.to_string()));
    assert!(!m.undecided_keys().contains(&ROCK.to_string()));
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
fn proceed_anyway_still_skips_what_is_undecided() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    let n = listed(&m).len();
    m.proceed_anyway();
    assert_eq!(m.dialog, None);
    idle(&mut m);
    assert_eq!(m.undecided(), n);
    assert!(m.export.summary.is_some());
    assert!(!d.path().join("export/landscape/leaf.dds").exists());
}

#[test]
fn proceed_anyway_after_a_row_waits_for_the_plan_of_that_decision() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    // The decision's plan is in flight: proceeding does not export on the stale plan.
    m.proceed_anyway();
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
    // "Proceed anyway" waits for the plan in flight; Export is clicked meanwhile and takes over.
    m.proceed_anyway();
    m.export_clicked();
    idle(&mut m);
    // The clicked export asked again (other textures are undecided); answering it exports once.
    assert!(matches!(m.dialog, Some(Dialog::ProceedAnyway(_))));
    m.proceed_anyway();
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

#[test]
fn rows_decided_in_the_dialog_go_back_through_the_export_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    // Emissive is on with no emissive texture: a block only once rock01 is a PBR set, which it is not yet.
    m.set_json_value(
        "landscape/rock01",
        "emissive",
        Some(serde_json::json!(true)),
    );
    idle(&mut m);
    m.export_clicked();
    assert!(!listed(&m).is_empty());
    // Rock01 becomes a PBR set; every other row is decided as its suggestion until the dialog empties.
    m.decide_undecided_row(ROCK, Subtype::PbrAlbedo);
    idle(&mut m);
    while let Some(Dialog::ProceedAnyway(keys)) = m.dialog.clone() {
        let k = keys[0].clone();
        let st = m.subtype_question(&k).flatten().unwrap_or(Subtype::Diffuse);
        m.decide_undecided_row(&k, st);
        idle(&mut m);
    }
    assert!(
        matches!(m.dialog, Some(Dialog::ExportBlocked(_))),
        "{:?}",
        m.dialog
    );
    assert!(m.export.running.is_none() && m.export.summary.is_none());
}
