//! Layout v2 Task 1.2: the Problems list in the model.
mod support;
use engine::codes::{Code, Warning};
use engine::config::Subtype;
use sdc_gui::model::bottom::{filtered, problems, problems_builds, At, Level, ProblemsFilter};
use sdc_gui::model::items::ItemStatus;
use sdc_gui::model::sets::set_views;
use sdc_gui::model::{Answer, AppModel};
use serde_json::json;
use std::time::Duration;
use support::*;

/// Every item of a PBR project carries exactly `ws` as its warnings (set_editor_ui.rs::with_only_warnings).
fn with_only_warnings(ws: &[Warning]) -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
    for s in m.sets.iter_mut() {
        s.warnings.clear();
    }
    let key = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
        .members[0]
        .clone();
    m.items.iter_mut().find(|i| i.key == key).unwrap().warnings = ws.to_vec();
    m.invalidate_views();
    (d, m)
}

/// `n` Ready textures cloned from a planned one (textures_filter.rs::many).
fn many(dir: &std::path::Path, n: usize) -> AppModel {
    let mut m = opened(dir);
    let base = m.items.iter().find(|i| i.job.is_some()).unwrap().clone();
    m.items = (0..n)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/t{n:05}");
            i.name = format!("t{n:05}.png");
            i.status = ItemStatus::Ready;
            i.warnings = vec![];
            i.set_key = None;
            i
        })
        .collect();
    m.sets = vec![];
    m.invalidate_views();
    m
}

#[test]
fn undecided_textures_are_decide_rows_and_a_decision_removes_its_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let p = problems(&m);
    let decide: Vec<_> = p.iter().filter(|p| p.level == Level::Decide).collect();
    assert_eq!(decide.len(), m.undecided());
    assert!(decide
        .iter()
        .all(|p| matches!(&p.at, At::Texture(k) if m.undecided_keys().contains(k))));
    let odd = At::Texture("landscape/odd.png".into());
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    assert!(m.wait_idle(Duration::from_secs(60)));
    // K8: the acknowledged code stays as a Warning row; only the Decide row goes.
    assert!(!problems(&m)
        .iter()
        .any(|p| p.level == Level::Decide && p.at == odd));
}

#[test]
fn a_decide_row_has_one_prefix() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    for p in problems(&m).iter().filter(|p| p.level == Level::Decide) {
        assert!(p.text.starts_with("Needs a decision: "), "{}", p.text);
        assert_eq!(p.text.matches("Needs a decision").count(), 1, "{}", p.text);
    }
}

#[test]
fn a_block_is_a_red_set_row_listed_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    let p = problems(&m);
    assert_eq!(p[0].level, Level::Blocks);
    assert!(matches!(&p[0].at, At::Set { id, .. } if id == "landscape/rock01"));
    assert!(p[0].text.contains("emissive"));
}

#[test]
fn a_pitfall_is_a_warning_row_of_its_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(problems(&m).iter().any(|p| p.level == Level::Warning
        && matches!(&p.at, At::Set { id, .. } if id == "landscape/rock01")));
}

#[test]
fn info_warnings_are_not_problems() {
    let (_d, m) = with_only_warnings(&[Warning::new(Code::AlphaPbrNormalDropped, "dropped")]);
    assert!(!problems(&m)
        .iter()
        .any(|p| p.code == Some(Code::AlphaPbrNormalDropped)));
    let (_d, m) = with_only_warnings(&[Warning::new(Code::LossReencode, "re-encoded")]);
    assert!(problems(&m)
        .iter()
        .any(|p| p.code == Some(Code::LossReencode)
            && p.level == Level::Warning
            && matches!(p.at, At::Texture(_))));
}

#[test]
fn a_failed_item_is_a_red_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.items[0].status = ItemStatus::Failed("no good".into());
    m.items[1].status = ItemStatus::Error(vec![Code::DimNpot, Code::LossReencode]);
    m.invalidate_views();
    let p = problems(&m);
    let red = |k: &str| {
        p.iter()
            .find(|p| p.level == Level::Blocks && p.at == At::Texture(k.into()))
            .map(|p| p.text.clone())
    };
    assert_eq!(red(&m.items[0].key).as_deref(), Some("Failed: no good"));
    assert_eq!(
        red(&m.items[1].key).as_deref(),
        Some(&*format!(
            "Failed: {}, {}",
            Code::DimNpot.as_str(),
            Code::LossReencode.as_str()
        ))
    );
}

#[test]
fn the_set_filter_keeps_the_sets_rows_and_its_members_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    let v = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap();
    let f = filtered(&m, Some(&ProblemsFilter::Set(v.key.clone())));
    assert!(!f.is_empty());
    assert!(f.iter().all(|p| match &p.at {
        At::Set { key, .. } => *key == v.key,
        At::Texture(k) => v.members.contains(k),
        _ => false,
    }));
    assert!(f.len() < problems(&m).len(), "other rows are filtered out");
    assert!(filtered(&m, Some(&ProblemsFilter::Undecided))
        .iter()
        .all(|p| p.level == Level::Decide));
    assert_eq!(filtered(&m, None).len(), problems(&m).len());
}

#[test]
fn the_unfiltered_list_is_the_problems_list_itself() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    assert!(std::sync::Arc::ptr_eq(&filtered(&m, None), &problems(&m)));
}

#[test]
fn open_problems_sets_the_filter_and_shows_the_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_problems(Some(ProblemsFilter::Undecided));
    assert!(m.bottom.open);
    assert_eq!(m.bottom.tab, sdc_gui::model::bottom::BottomTab::Problems);
    assert_eq!(m.bottom.filter, Some(ProblemsFilter::Undecided));
}

#[test]
fn problems_are_built_once_per_change_at_10k() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many(d.path(), 10_000);
    let n = problems_builds(&m);
    for _ in 0..5 {
        let _ = problems(&m);
    }
    assert_eq!(problems_builds(&m), n + 1);
    m.invalidate_views();
    let _ = problems(&m);
    let _ = problems(&m);
    assert_eq!(problems_builds(&m), n + 2);
}
