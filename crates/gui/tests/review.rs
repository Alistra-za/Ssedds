//! Layout v2 Task 1.3 (B2, C2, C5; rulings R1–R4): Review replaces the undecided, blocked and pitfall modals.
mod support;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::*;
use serde_json::json;
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

#[test]
fn a_clean_project_exports_without_review() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.review.is_none() && m.dialog.is_none(), "{:?}", m.dialog);
    idle(&mut m);
    assert!(m.export.summary.is_some());
}

#[test]
fn undecided_textures_open_review_not_a_dialog() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    assert!(m.review.is_some());
    assert_eq!(m.dialog, None);
    assert_eq!(m.bottom.tab, BottomTab::Review);
    assert!(m.bottom.open);
    let (_, skipped) = m
        .review_gate()
        .expect("undecided textures never disable Export");
    assert_eq!(skipped, m.undecided());
    assert!(
        m.review_button_text()
            .contains(&format!("(skip {} undecided)", m.undecided())),
        "{}",
        m.review_button_text()
    );
}

#[test]
fn a_blocker_disables_review_export_with_its_reason() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m.export_clicked();
    assert!(m.review.is_some());
    assert_eq!(m.review_gate(), Err("Fix 1 blocker first".to_string()));
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_none() && m.review.is_some());
}

/// Was pbr_export.rs::blocks_never_offer_proceed_anyway: a block and a pitfall, the block wins.
#[test]
fn a_blocker_wins_over_a_pitfall() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    idle(&mut m);
    assert!(!pbr_export::export_asks(&m).is_empty());
    m.export_clicked();
    assert!(m.review.is_some());
    m.acknowledge_all_asks();
    assert_eq!(m.review_gate(), Err("Fix 1 blocker first".to_string()));
}

#[test]
fn pitfalls_gate_until_acknowledged_and_only_for_this_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    m.export_clicked();
    // match8 here raises two pitfalls: its `_s` list, and match slots that point at different textures.
    assert_eq!(
        m.review_gate(),
        Err("Acknowledge 2 warnings first".to_string())
    );
    m.acknowledge_all_asks();
    assert!(m.review_gate().is_ok());
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.summary.is_some());
    // The next export asks again.
    m.export_clicked();
    assert!(m.review.is_some());
    assert!(m.review_gate().is_err());
}

#[test]
fn one_tick_acknowledges_one_pitfall_and_can_be_taken_back() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    m.export_clicked();
    let texts: Vec<String> = pbr_export::export_asks(&m)
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    m.acknowledge_ask(&texts[0], true);
    assert_eq!(
        m.review_gate(),
        Err("Acknowledge 1 warning first".to_string())
    );
    m.acknowledge_ask(&texts[1], true);
    assert!(m.review_gate().is_ok());
    m.acknowledge_ask(&texts[1], false);
    assert_eq!(
        m.review_gate(),
        Err("Acknowledge 1 warning first".to_string())
    );
}

/// Was pbr_export.rs::pitfalls_ask_and_proceeding_continues_to_the_undecided_check: the pitfall and the undecided
/// textures are one Review; acknowledging lets Export skip the undecided ones.
#[test]
fn pitfalls_and_undecided_textures_share_one_review() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    assert!(m.undecided() > 0);
    m.export_clicked();
    assert!(m.review.is_some() && m.dialog.is_none(), "{:?}", m.dialog);
    m.acknowledge_all_asks();
    let (_, skipped) = m.review_gate().unwrap();
    assert_eq!(skipped, m.undecided());
}

/// Was pbr_export.rs::proceeding_with_warnings_waits_for_a_plan_in_flight (audit 08f-2).
#[test]
fn acknowledged_pitfalls_export_waits_for_a_plan_in_flight() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    m.export_clicked();
    m.acknowledge_all_asks();
    m.replan();
    m.export_from_review();
    assert!(m.export_queued() && m.export.running.is_none());
    idle(&mut m);
    assert!(m.export.summary.is_some());
}

#[test]
fn a_new_pitfall_after_the_click_stops_the_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    m.export_clicked();
    m.acknowledge_all_asks();
    m.replan();
    m.export_from_review();
    assert!(m.export_queued());
    // A second, different pitfall (alpha flags without Create NiAlphaProperty) lands before the plan does.
    m.set_json_value("landscape/rock01", "alphaprop_thresh", Some(json!(128)));
    idle(&mut m);
    assert!(m.export.summary.is_none() && m.export.running.is_none());
    assert!(m.review.is_some());
    assert!(
        m.status_line
            .starts_with("Export not started: Acknowledge 1 warning first"),
        "{}",
        m.status_line
    );
}

#[test]
fn export_from_review_waits_for_a_plan_in_flight() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    // A plan starts.
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    m.export_from_review();
    assert!(m.export_queued() && m.export.running.is_none());
    idle(&mut m);
    let s = m
        .export
        .summary
        .clone()
        .expect("exported on the settled plan");
    assert!(
        s.skipped > 0,
        "the other undecided textures were skipped (N3)"
    );
    assert!(d.path().join("export/landscape/rock01.dds").exists());
}

#[test]
fn export_from_review_reruns_the_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    answer_all_but_subtypes(&mut m);
    // Emissive is on with no emissive texture: a block only once rock01 is a PBR set, which it is not yet.
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    idle(&mut m);
    m.export_clicked();
    assert!(m.review.is_some());
    m.decide_undecided_row(ROCK, Subtype::PbrAlbedo);
    idle(&mut m);
    while let Some(k) = m.undecided_keys().first().cloned() {
        let st = m.subtype_question(&k).flatten().unwrap_or(Subtype::Diffuse);
        m.decide_undecided_row(&k, st);
        idle(&mut m);
    }
    m.export_from_review();
    idle(&mut m);
    assert!(m.export.running.is_none() && m.export.summary.is_none());
    assert!(m.review_gate().unwrap_err().starts_with("Fix "));
}

#[test]
fn export_from_review_plans_again_when_a_source_changed() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    let p = m.root.clone().unwrap().join(LEAF);
    image::ImageBuffer::from_fn(32, 32, |_, _| image::Rgba([9u8, 9, 9, 255]))
        .save(&p)
        .unwrap();
    m.export_from_review();
    assert!(m.export_queued());
    idle(&mut m);
    assert!(m.export.summary.is_some());
    assert!(m.items.iter().all(|i| !i.changed_on_disk));
}

#[test]
fn deciding_the_last_undecided_texture_never_exports_by_itself() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.accept_all_suggestions();
    idle(&mut m);
    // Accept all leaves the questions that are not a sub-type.
    m.answer_undecided_row(LEAF, Answer::Alpha(AlphaPolicy::Cutout));
    m.answer_undecided_row(ODD, Answer::Acknowledge(Code::DimNpot));
    m.answer_undecided_row(NORMAL, Answer::Green(GreenConvention::Directx));
    idle(&mut m);
    assert_eq!(m.undecided(), 0, "{:?}", m.undecided_keys());
    assert!(m.export.summary.is_none() && m.review.is_some());
    assert_eq!(
        m.review_button_text(),
        format!("Export {} files", m.review_gate().unwrap().0)
    );
}

#[test]
fn opening_another_folder_closes_review_without_exporting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    // A plan in flight, so the click below is queued.
    m.replan();
    m.export_from_review();
    assert!(m.export_queued());
    let d2 = tempfile::tempdir().unwrap();
    let root2 = mod_folder(d2.path());
    assert!(m.request_open(root2.clone()));
    idle(&mut m);
    assert!(m.review.is_none());
    assert!(!(m.bottom.open && m.bottom.tab == BottomTab::Review));
    assert!(m.export.summary.is_none());
    assert!(!root2.join("sdc.toml").exists());
}

/// K4: Reload drops a queued Review export and closes Review.
#[test]
fn reload_closes_review_and_drops_its_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.replan();
    m.export_from_review();
    assert!(m.export_queued());
    m.reload_documents();
    assert!(!m.export_queued());
    idle(&mut m);
    assert!(m.review.is_none());
    assert!(m.export.summary.is_none() && m.export.running.is_none());
}

#[test]
fn undo_reverts_a_decision_made_in_review() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.decide_undecided_row(ROCK, Subtype::Diffuse);
    idle(&mut m);
    assert!(!m.undecided_keys().contains(&ROCK.to_string()));
    m.undo();
    idle(&mut m);
    assert!(m.undecided_keys().contains(&ROCK.to_string()));
    assert!(m.review.is_some() && m.export.summary.is_none());
}

#[test]
fn the_gate_names_a_read_only_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    assert!(m.review.is_some());
    assert_eq!(m.fix_off(), None);
    std::fs::write(m.root.clone().unwrap().join("sdc.toml"), "nonsense = [").unwrap();
    m.reload_documents();
    idle(&mut m);
    m.open_review();
    let why = m.review_gate().unwrap_err();
    assert!(why.starts_with("sdc.toml is read-only: "), "{why}");
    assert_eq!(m.fix_off(), Some(why));
}

#[test]
fn fix_buttons_wait_for_a_plan_in_flight() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.replan();
    assert_eq!(m.fix_off().as_deref(), Some("Wait for the plan to finish"));
    idle(&mut m);
    assert_eq!(m.fix_off(), None);
}

#[test]
fn closing_review_collapses_the_panel_it_showed() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    assert!(m.bottom.open);
    m.close_review();
    assert!(m.review.is_none() && !m.bottom.open);
}

// ---- ported from export_undecided.rs (the undecided list is Review's now)

/// Every non-sub-type question answered, so only the sub-types are still undecided.
fn answer_all_but_subtypes(m: &mut AppModel) {
    m.decide(LEAF, Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(ODD, Answer::Acknowledge(Code::DimNpot));
    m.decide(NORMAL, Answer::Green(GreenConvention::Directx));
    idle(m);
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
    assert!(m.review.is_some());
    m.accept_all_suggestions();
    assert!(m.undecided_keys().contains(&LEAF.to_string()));
    assert!(m.export.running.is_none());
    idle(&mut m);
    assert!(m.undecided_keys().contains(&LEAF.to_string()));
    assert!(!m.undecided_keys().contains(&ROCK.to_string()));
}

// ---- fix round 1

/// A scan read that lands before the plan in flight (no scan DB: it lands at once, unchanged) never starts a queued
/// Review export on the stale plan; the export waits for the plan.
#[test]
fn a_scan_read_landing_before_the_plan_never_starts_a_queued_review_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    assert!(m.review.is_some());
    m.replan();
    m.export_from_review();
    assert!(m.export_queued());
    m.refresh_mesh_votes(false);
    assert!(
        m.export.running.is_none(),
        "started while a plan was in flight"
    );
    assert!(m.export_queued());
    idle(&mut m);
    assert!(m.export.summary.is_some(), "exported on the settled plan");
}

/// Closing Review (Task 1.4's Close) drops its queued export.
#[test]
fn closing_review_drops_its_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.replan();
    m.export_from_review();
    assert!(m.export_queued());
    m.close_review();
    assert!(!m.export_queued());
    idle(&mut m);
    assert!(m.export.summary.is_none() && m.export.running.is_none());
}

/// Review's Export does nothing while Review is closed.
#[test]
fn export_from_review_needs_review_open() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.review.is_none());
    m.replan();
    m.export_from_review();
    assert!(!m.export_queued());
    idle(&mut m);
    assert!(m.export.summary.is_none() && m.export.running.is_none());
}
