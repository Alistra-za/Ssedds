//! M6 Track F3b Task 2: a set's outputs take a new base name (`output_name`), and a PBR set on CS renames through
//! its PGPatcher `rename` key (spec §3).
mod support;
use engine::config::Target;
use engine::sets::OutputKey;
use sdc_gui::model::sets::*;
use sdc_gui::model::AppModel;
use std::path::PathBuf;
use std::time::Duration;
use support::*;

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

fn key_of(m: &AppModel, name: &str) -> String {
    m.items
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("no item {name}"))
        .key
        .clone()
}

fn output_of(m: &AppModel, name: &str) -> String {
    let it = m.items.iter().find(|i| i.name == name).unwrap();
    it.job
        .as_ref()
        .unwrap()
        .output
        .to_string_lossy()
        .replace('\\', "/")
}

fn entry(m: &AppModel, id: &str) -> sdc_gui::sets_doc::SetEntry {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .cloned()
        .find(|e| engine::sets::same_id(&e.id, id))
        .unwrap_or_else(|| panic!("no entry {id}"))
}

#[test]
fn an_output_name_renames_a_plain_set_on_cs() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_output_name("landscape/leaf", Some("maple")).unwrap();
    idle(&mut m);
    assert_eq!(
        entry(&m, "landscape/leaf").output_name.as_deref(),
        Some("maple")
    );
    assert_eq!(
        engine_inputs(&m)
            .1
            .get(&OutputKey::File(key_of(&m, "leaf.png"))),
        Some(&PathBuf::from("landscape/maple.dds"))
    );
    assert!(output_of(&m, "leaf.png").ends_with("export/landscape/maple.dds"));
    assert!(d.path().join("textures/landscape/leaf.png").exists());
    // One step back restores the mirrored name.
    m.undo();
    idle(&mut m);
    assert!(output_of(&m, "leaf.png").ends_with("export/landscape/leaf.dds"));
}

#[test]
fn every_member_keeps_its_suffix_on_vanilla() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    m.set_output_name("landscape/rock01", Some("boulder"))
        .unwrap();
    idle(&mut m);
    assert!(output_of(&m, "rock01.png").ends_with("export/landscape/boulder.dds"));
    assert!(output_of(&m, "rock01_normal.png").ends_with("export/landscape/boulder_n.dds"));
    // Planning again changes nothing: the name is applied to its own result idempotently.
    let before = engine_inputs(&m).1;
    m.replan();
    idle(&mut m);
    assert_eq!(engine_inputs(&m).1, before);
    // Clearing restores the mirrored names.
    m.set_output_name("landscape/rock01", None).unwrap();
    idle(&mut m);
    assert_eq!(entry(&m, "landscape/rock01").output_name, None);
    assert!(output_of(&m, "rock01.png").ends_with("export/landscape/rock01.dds"));
}

#[test]
fn a_pbr_set_on_cs_renames_through_pgpatcher_rename() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_output_name("landscape/rock01", Some("steel"))
        .unwrap();
    idle(&mut m);
    let e = entry(&m, "landscape/rock01");
    assert_eq!(e.output_name, None);
    assert_eq!(e.settings.get("rename"), Some(&serde_json::json!("steel")));
    let (_, paths) = engine_inputs(&m);
    assert_eq!(
        paths.get(&OutputKey::File(key_of(&m, "rock01.png"))),
        Some(&PathBuf::from("textures/pbr/landscape/steel.dds"))
    );
    assert_eq!(
        paths.get(&OutputKey::Packed {
            set: "landscape/rock01".into(),
            suffix: "rmaos".into()
        }),
        Some(&PathBuf::from("textures/pbr/landscape/steel_rmaos.dds"))
    );
    m.set_output_name("landscape/rock01", None).unwrap();
    idle(&mut m);
    assert_eq!(entry(&m, "landscape/rock01").settings.get("rename"), None);
}

#[test]
fn output_names_windows_cannot_use_or_that_end_in_a_suffix_are_refused() {
    for bad in [
        "wall_n", "x_rmaos", "CON", "com1.tga", "a/b", "a\\b", "x.", "x ", "x?y", "..",
    ] {
        assert!(check_output_name(bad).is_err(), "{bad}");
    }
    for good in ["steel", "iron_plate", "Boulder 02", "n"] {
        assert!(check_output_name(good).is_ok(), "{good}");
    }
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.set_output_name("landscape/leaf", Some("leaf_n")).is_err());
    assert_eq!(entry_or_none(&m, "landscape/leaf"), None);
}

fn entry_or_none(m: &AppModel, id: &str) -> Option<String> {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .cloned()
        .find(|e| engine::sets::same_id(&e.id, id))
        .and_then(|e| e.output_name)
}

#[test]
fn renamed_output_keeps_folder_and_suffix() {
    assert_eq!(
        renamed_output("armor/wall_n.dds", "steel"),
        Some(PathBuf::from("armor/steel_n.dds"))
    );
    assert_eq!(
        renamed_output("wall.dds", "steel"),
        Some(PathBuf::from("steel.dds"))
    );
    assert_eq!(
        renamed_output("a/b/rock_rmaos.dds", "x"),
        Some(PathBuf::from("a/b/x_rmaos.dds"))
    );
    // An exporter term an output keeps stays too, so it never lands on the diffuse's name.
    assert_eq!(
        renamed_output("a/rock01_roughness.dds", "x"),
        Some(PathBuf::from("a/x_roughness.dds"))
    );
}
