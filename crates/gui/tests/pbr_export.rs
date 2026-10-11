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

fn texts(m: &sdc_gui::model::AppModel) -> Vec<String> {
    export_blocks(m).into_iter().map(|i| i.text).collect()
}

#[test]
fn feature_rows_use_the_exact_text() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let id = "landscape/rock01";
    m.set_json_value(id, "emissive", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: emissive is on but the set has no emissive texture (_g)"
        )]
    );
    m.set_json_value(id, "emissive", None);
    m.set_json_value(id, "parallax", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: parallax is on but the set has no height texture (_p)"
        )]
    );
    m.set_json_value(id, "parallax", None);
    m.set_json_value(id, "subsurface", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: subsurface is on but the set has no subsurface texture (_s)"
        )]
    );
    m.set_json_value(id, "subsurface", None);
    m.set_json_value(id, "multilayer", Some(json!(true)));
    m.set_json_value(id, "coat_normal", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: coat normal is on but the set has no coat normal texture (_cnr)"
        )]
    );
}

#[test]
fn fuzz_texture_row_and_a_filled_slot() {
    // `fuzz` and `fuzz.texture` cannot both be written through the TOML document today (a bool and a table share
    // the key), so this row is checked on the settings directly.
    use sdc_gui::model::sets::SlotKind;
    let mut s = sdc_gui::pgp::settings::Settings::new();
    s.insert("fuzz".into(), json!(true));
    s.insert("fuzz.texture".into(), json!(true));
    assert_eq!(
        missing_texture_texts("a/b", &s, |_| false),
        vec!["a/b: fuzz texture is on but the set has no fuzz texture (_f)".to_string()]
    );
    assert!(missing_texture_texts("a/b", &s, |k| k == SlotKind::Fuzz).is_empty());
}

#[test]
fn only_the_matching_slot_silences_a_feature() {
    use sdc_gui::model::sets::SlotKind;
    let one = |key: &str| {
        let mut s = sdc_gui::pgp::settings::Settings::new();
        s.insert(key.into(), json!(true));
        s
    };
    for (key, slot) in [
        ("emissive", SlotKind::Emissive),
        ("parallax", SlotKind::Height),
        ("subsurface", SlotKind::Subsurface),
    ] {
        let s = one(key);
        assert_eq!(missing_texture_texts("x", &s, |_| false).len(), 1, "{key}");
        assert!(
            missing_texture_texts("x", &s, |k| k == slot).is_empty(),
            "{key}"
        );
        let others = |k: SlotKind| k != slot;
        assert_eq!(missing_texture_texts("x", &s, others).len(), 1, "{key}");
    }
}

#[test]
fn coat_and_fuzz_clash_on_any_set() {
    // landscape/zed has an undecided diffuse (so it is not a PBR set) and both a _cnr and an _f.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    for n in ["zed.png", "zed_cnr.png", "zed_f.png"] {
        image::ImageBuffer::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 9u8]))
            .save(r.join(n))
            .unwrap();
    }
    let m = ready(d.path());
    assert!(set_views(&m)
        .iter()
        .any(|v| v.id == "landscape/zed" && !v.pbr));
    assert_eq!(
        texts(&m),
        vec!["landscape/zed: _cnr and _f both use slot 7".to_string()]
    );
}

#[test]
fn non_pbr_sets_and_vanilla_skip_the_pbr_checks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    // leaf is not a PBR set: a feature with no texture is not a block there.
    m.set_json_value("landscape/leaf", "emissive", Some(json!(true)));
    assert_eq!(export_blocks(&m), vec![]);
    // Vanilla: even a PBR set is not checked, and nothing asks.
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    assert!(!export_blocks(&m).is_empty());
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    assert_eq!(export_asks(&m), vec![]);
    assert!(json_entries(&m).is_empty());
}

#[test]
fn blocks_never_offer_proceed_anyway() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    // Both a block and a pitfall: the block wins and the dialog is the blocked one.
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    idle(&mut m);
    assert!(!export_asks(&m).is_empty());
    m.export_clicked();
    assert!(
        matches!(m.dialog, Some(Dialog::ExportBlocked(_))),
        "{:?}",
        m.dialog
    );
}

#[test]
fn the_json_name_must_be_one_usable_file_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    for bad in [
        r"a\b", "a/b", "..", "x..y", "name.", "name ", "CON", "nul", "Com1", "lpt9.txt", "a:b",
        "a?b",
    ] {
        m.sets_doc.as_mut().unwrap().set_json_name(Some(bad));
        let b = texts(&m);
        assert!(
            b.iter().any(|t| t.starts_with("JSON name:")),
            "{bad:?} should block: {b:?}"
        );
    }
    for good in ["textures", "My Pack", "com10", "console"] {
        m.sets_doc.as_mut().unwrap().set_json_name(Some(good));
        assert_eq!(export_blocks(&m), vec![], "{good:?} should be fine");
    }
}

#[test]
fn the_json_name_is_not_checked_when_there_is_nothing_to_write() {
    let d = tempfile::tempdir().unwrap();
    // No set is decided as PBR, so there is no JSON to write and its name does not matter.
    let mut m = opened(d.path());
    assert!(set_views(&m).iter().all(|v| !v.pbr));
    m.sets_doc.as_mut().unwrap().set_json_name(Some("CON"));
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_borrow_whose_owner_is_blocked_is_reported_once_by_the_owners_block() {
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
    m.drop_choice(&rmaos, &id, DropChoice::Share);
    m.reorder_set("landscape/rock01", 0);
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![], "the borrow resolves to rock01");
    m.set_json_value("landscape/rock01", "rename", Some(json!(r"..\x")));
    idle(&mut m);
    let b = texts(&m);
    assert!(b.iter().any(|t| t.contains("rename")), "{b:?}");
    assert!(!b.iter().any(|t| t.contains("borrows")), "{b:?}");
}
