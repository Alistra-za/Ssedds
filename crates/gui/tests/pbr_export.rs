mod support;
use sdc_gui::model::pbr_export::*;
use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome};
use sdc_gui::model::Dialog;
use serde_json::json;
use std::time::Duration;
use support::*;

fn ready(d: &std::path::Path) -> sdc_gui::model::AppModel {
    let mut m = pbr_opened(d);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

#[test]
fn a_clean_project_has_no_blocks() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_feature_without_its_texture_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.set_id == "landscape/rock01" && i.text.contains("emissive")),
        "{b:?}"
    );
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::ExportBlocked(_))));
}

#[test]
fn exclusive_features_in_one_set_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "hair", Some(json!(true)));
    m.set_json_value("landscape/rock01", "parallax", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("hair") && i.text.contains("parallax")),
        "{b:?}"
    );
}

#[test]
fn a_bad_rename_blocks_and_names_the_field() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "rename", Some(json!(r"..\escape")));
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("rename") && i.text.contains("..")),
        "{b:?}"
    );
}

#[test]
fn a_bad_slot_path_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "slot3", Some(json!(r"..\g.dds")));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("slot3") && i.text.contains("..")),
        "{b:?}"
    );
}

#[test]
fn two_sets_deriving_one_prefix_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    // Point leaf's set at rock01's match: both diffuse outputs land on rock01.dds.
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    m.set_json_value(
        "landscape/leaf",
        "match_diffuse",
        Some(json!(r"landscape\rock01")),
    );
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("both write")), "{b:?}");
}

#[test]
fn an_owned_output_on_a_mirrored_path_blocks() {
    // The engine skips most such collisions itself (and the plan then fails to settle); this is the check for the
    // ones it does not re-check. Simulate that plan: leaf.png mirrors to the very file rock01 diffuse is sent to.
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    assert_eq!(path_collisions(&m), vec![]);
    let leaf = m.items.iter_mut().find(|i| i.name == "leaf.png").unwrap();
    assert!(leaf.job.is_some(), "the fixture leaf has a planned job");
    leaf.job_key = "textures/pbr/landscape/rock01.dds".into();
    let b = path_collisions(&m);
    assert!(
        b.iter().any(|i| i.set_id == "landscape/rock01"
            && i.text.contains("textures/pbr/landscape/rock01.dds")
            && i.text.contains("landscape/leaf.png")),
        "{b:?}"
    );
    assert!(export_blocks(&m)
        .iter()
        .any(|i| i.text.contains("leaf.png")));
}

#[test]
fn pitfalls_ask_and_proceeding_continues_to_the_undecided_check() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    assert!(!export_asks(&m).is_empty());
    m.export_clicked();
    assert!(
        matches!(m.dialog, Some(Dialog::ExportWarnings(_))),
        "{:?}",
        m.dialog
    );
    m.proceed_with_warnings();
    // The fixture has undecided textures: the M3a question comes next.
    assert!(
        matches!(m.dialog, Some(Dialog::ProceedAnyway(_))),
        "{:?}",
        m.dialog
    );
}

#[test]
fn the_json_of_the_fixture_validates_and_names_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    let entries = json_entries(&m);
    assert!(entries.iter().any(|(v, _)| v.id == "landscape/rock01"));
    let doc =
        sdc_gui::pgp::writer::document(&entries.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>());
    sdc_gui::pgp::validate::validate(&doc.to_value()).unwrap();
    assert_eq!(json_name(&m), "textures", "the opened folder's name");
}

#[test]
fn a_borrowed_slot_is_written_as_the_owners_output_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert!(matches!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    m.reorder_set("landscape/rock01", 0);
    idle(&mut m);
    assert!(set_views(&m).iter().any(|v| v.id == id && v.pbr));
    let (_, e) = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == id)
        .unwrap();
    assert_eq!(
        e.slots.get(&6).map(String::as_str),
        Some(r"textures\pbr\landscape\rock01_rmaos.dds")
    );
}
