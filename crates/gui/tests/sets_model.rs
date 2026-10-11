mod support;
use sdc_gui::model::sets::*;
use serde_json::json;
use std::time::Duration;
use support::*;

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

fn view<'a>(v: &'a [SetView], id: &str) -> &'a SetView {
    v.iter().find(|s| s.id == id).unwrap_or_else(|| {
        panic!(
            "no set {id} in {:?}",
            v.iter().map(|s| &s.id).collect::<Vec<_>>()
        )
    })
}

fn key_of(m: &sdc_gui::model::AppModel, name: &str) -> String {
    m.items
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("no item {name}"))
        .key
        .clone()
}

#[test]
fn the_fixture_set_has_its_slots_a_derived_match_and_pbr_output_paths() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m); // the second (output-path) plan
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    assert!(
        rock.pbr,
        "rock01 packs _rmaos, so its diffuse resolves pbr_albedo"
    );
    assert!(matches!(
        rock.slots.get(&SlotKind::Diffuse),
        Some(SlotFill::Own { .. })
    ));
    assert!(matches!(
        rock.slots.get(&SlotKind::Normal),
        Some(SlotFill::Own { .. })
    ));
    assert!(matches!(
        rock.slots.get(&SlotKind::Rmaos),
        Some(SlotFill::Own { .. })
    ));
    assert_eq!(rock.derived_match.as_deref(), Some(r"landscape\rock01"));
    assert_eq!(
        rock.prefix.as_deref(),
        Some(r"textures\pbr\landscape\rock01")
    );
    let (_, paths) = engine_inputs(&m);
    use engine::sets::OutputKey;
    assert_eq!(
        paths.get(&OutputKey::File(key_of(&m, "rock01.png"))),
        Some(&std::path::PathBuf::from(
            "textures/pbr/landscape/rock01.dds"
        ))
    );
    assert_eq!(
        paths.get(&OutputKey::Packed {
            set: "landscape/rock01".into(),
            suffix: "rmaos".into()
        }),
        Some(&std::path::PathBuf::from(
            "textures/pbr/landscape/rock01_rmaos.dds"
        ))
    );
    // The plan the model holds now writes there.
    let diffuse = m.items.iter().find(|i| i.name == "rock01.png").unwrap();
    assert!(diffuse
        .job
        .as_ref()
        .unwrap()
        .output
        .to_string_lossy()
        .replace('\\', "/")
        .ends_with("textures/pbr/landscape/rock01.dds"));
}

#[test]
fn vanilla_gets_no_output_paths() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    assert!(engine_inputs(&m).1.is_empty());
}

#[test]
fn dropping_a_diffuse_on_a_set_with_one_asks_to_replace() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let leaf = key_of(&m, "leaf.png");
    let out = m.drop_on_set(&leaf, "landscape/rock01");
    assert_eq!(
        out,
        DropOutcome::AskReplace {
            existing: key_of(&m, "rock01.png")
        }
    );
    m.drop_replace(&leaf, "landscape/rock01");
    idle(&mut m);
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    assert!(
        matches!(rock.slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if *item == leaf)
    );
    // The replaced diffuse became a set of its own; one undo puts both back.
    m.undo();
    idle(&mut m);
    let v = set_views(&m);
    assert!(
        matches!(view(&v, "landscape/rock01").slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if *item == key_of(&m, "rock01.png"))
    );
}

#[test]
fn new_set_share_and_reorder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    assert!(set_views(&m).iter().any(|s| s.id == id && s.created));
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert_eq!(
        m.drop_on_set(&rmaos, &id),
        DropOutcome::AskChoice {
            from: "landscape/rock01".into()
        }
    );
    assert_eq!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    let v = set_views(&m);
    assert!(
        matches!(view(&v, &id).slots.get(&SlotKind::Rmaos), Some(SlotFill::Borrowed { from, .. }) if from == "landscape/rock01")
    );
    // rock01 is the first PBR set using the shared output: it owns it.
    assert_eq!(view(&v, "landscape/rock01").owns, vec![SlotKind::Rmaos]);
    // Reorder puts the new set first.
    m.reorder_set(&id, 0);
    assert_eq!(set_views(&m)[0].id, id);
    assert_eq!(m.sets_doc.as_ref().unwrap().order()[0], id);
}

#[test]
fn a_packed_output_cannot_be_moved_only_shared() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = key_of(&m, "rock01_rmaos");
    assert!(matches!(
        m.drop_choice(&rmaos, &id, DropChoice::Move),
        DropOutcome::Refused(_)
    ));
    assert!(m.remove_member("landscape/rock01", &rmaos).is_err());
}

#[test]
fn duplicating_a_raw_source_copies_it_and_undo_deletes_it_only_when_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap(); // e.g. "landscape/odd-2"
    idle(&mut m);
    let rough = key_of(&m, "rock01_roughness.png");
    assert_eq!(
        m.drop_on_set(&rough, &id),
        DropOutcome::AskChoice {
            from: "landscape/rock01".into()
        }
    );
    assert!(
        matches!(
            m.drop_choice(&rough, &id, DropChoice::Share),
            DropOutcome::Refused(_)
        ),
        "raw maps are not shareable"
    );
    assert_eq!(
        m.drop_choice(&rough, &id, DropChoice::Duplicate),
        DropOutcome::Joined
    );
    let base = id.rsplit('/').next().unwrap().to_string();
    let copy = m
        .root
        .clone()
        .unwrap()
        .join(format!("landscape/{base}_roughness.png"));
    assert!(copy.exists(), "{copy:?}");
    m.undo();
    assert!(!copy.exists(), "an unchanged copy is deleted by undo");
    m.redo();
    assert!(copy.exists(), "redo copies it again");
    std::fs::write(&copy, b"changed").unwrap();
    m.undo();
    assert!(copy.exists(), "a changed copy is kept");
    assert!(matches!(m.dialog, Some(sdc_gui::model::Dialog::Error(_))));
}

#[test]
fn coat_and_fuzz_cannot_share_a_set() {
    assert!(slot_conflict(&[SlotKind::Fuzz], SlotKind::Coat).is_some());
    assert!(slot_conflict(&[SlotKind::Coat], SlotKind::Fuzz).is_some());
    assert!(slot_conflict(&[SlotKind::Diffuse, SlotKind::Normal], SlotKind::Coat).is_none());
}

#[test]
fn slot_numbers_follow_p6() {
    let n: Vec<(SlotKind, u8, &str)> = SlotKind::ALL
        .iter()
        .map(|k| (*k, k.slot_number(), k.suffix()))
        .collect();
    assert_eq!(
        n,
        vec![
            (SlotKind::Diffuse, 1, ""),
            (SlotKind::Normal, 2, "n"),
            (SlotKind::Rmaos, 6, "rmaos"),
            (SlotKind::Height, 4, "p"),
            (SlotKind::Emissive, 3, "g"),
            (SlotKind::Subsurface, 8, "s"),
            (SlotKind::Coat, 7, "cnr"),
            (SlotKind::Fuzz, 7, "f"),
        ]
    );
}

#[test]
fn a_member_that_vanished_shows_as_missing_and_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.edit_docs("t", None, true, |_, s| {
        s.add_member("landscape/rock01", "landscape/gone.png")
    });
    idle(&mut m);
    let v = set_views(&m);
    assert_eq!(
        view(&v, "landscape/rock01").missing,
        vec!["landscape/gone.png".to_string()]
    );
    assert!(
        m.sets_doc
            .as_ref()
            .unwrap()
            .entries()
            .iter()
            .any(|e| e.add.contains(&"landscape/gone.png".to_string())),
        "never dropped"
    );
}

#[test]
fn edits_go_creates_and_renames_first_then_moves_then_removes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.edit_docs("t", None, false, |_, s| {
        s.add_member("mine/new", "landscape/leaf.png");
        s.create_set("mine/new");
        s.remove_member("landscape/rock01", "landscape/rock01_normal.png");
    });
    let (edits, _) = engine_inputs(&m);
    use engine::sets::SetEdit;
    assert!(matches!(&edits[0], SetEdit::Create { set } if set == "mine/new"));
    assert!(matches!(&edits[1], SetEdit::Move { to, .. } if to == "mine/new"));
    assert!(matches!(edits.last().unwrap(), SetEdit::Remove { .. }));
}

#[test]
fn json_only_edits_do_not_replan_but_match_edits_do() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value(
        Some("landscape/rock01"),
        "specular_level",
        Some(json!(0.05)),
    );
    assert!(!m.busy(), "a material value does not plan");
    m.set_json_value(
        Some("landscape/rock01"),
        "match_diffuse",
        Some(json!(r"landscape\boulder")),
    );
    assert!(m.busy(), "a match changes output paths, so it plans");
    idle(&mut m);
    assert_eq!(
        view(&set_views(&m), "landscape/rock01").prefix.as_deref(),
        Some(r"textures\pbr\landscape\boulder")
    );
}

#[test]
fn clearing_a_match_returns_to_the_derived_prefix() {
    // The derived match comes from the source, not from the (redirected) output, so it never follows a match.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value(
        Some("landscape/rock01"),
        "match_diffuse",
        Some(json!(r"landscape\boulder")),
    );
    idle(&mut m);
    let diffuse = m.items.iter().find(|i| i.name == "rock01.png").unwrap();
    assert!(diffuse
        .job
        .as_ref()
        .unwrap()
        .output
        .to_string_lossy()
        .replace('\\', "/")
        .ends_with("textures/pbr/landscape/boulder.dds"));
    assert_eq!(
        view(&set_views(&m), "landscape/rock01")
            .derived_match
            .as_deref(),
        Some(r"landscape\rock01")
    );
    m.set_json_value(Some("landscape/rock01"), "match_diffuse", None);
    idle(&mut m);
    assert_eq!(
        view(&set_views(&m), "landscape/rock01").prefix.as_deref(),
        Some(r"textures\pbr\landscape\rock01")
    );
}

#[test]
fn removing_the_only_member_of_a_set_does_nothing() {
    // The engine would make it a set of its own named `<stem>~2` (it is already one).
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let leaf = key_of(&m, "leaf.png");
    let before = m.sets_doc.as_ref().unwrap().text();
    let steps = m.history.undo.len();
    assert!(m.remove_member("landscape/leaf", &leaf).is_err());
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
    assert_eq!(m.history.undo.len(), steps);
    // The only member of a set it was moved into goes back to the automatic grouping (no Remove edit).
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    assert!(m.remove_member(&id, &odd).is_ok());
    idle(&mut m);
    let v = set_views(&m);
    assert!(view(&v, "landscape/odd").members.contains(&odd));
    assert!(
        view(&v, &id).members.is_empty(),
        "the created set stays, empty"
    );
    assert!(engine_inputs(&m)
        .0
        .iter()
        .all(|e| !matches!(e, engine::sets::SetEdit::Remove { .. })));
    // A member of a bigger set leaves it.
    let normal = key_of(&m, "rock01_normal.png");
    assert!(m.remove_member("landscape/rock01", &normal).is_ok());
    idle(&mut m);
    assert!(!view(&set_views(&m), "landscape/rock01")
        .members
        .contains(&normal));
}

#[test]
fn rename_refuses_names_in_use_and_bad_names() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    assert!(m.rename_set("landscape/rock01", "landscape/leaf").is_err());
    assert!(m.rename_set("landscape/rock01", "LANDSCAPE/Leaf").is_err());
    assert!(m.rename_set("landscape/rock01", "../up").is_err());
    assert!(m.rename_set("landscape/rock01", "a|b").is_err());
    assert!(m.rename_set("landscape/rock01", "").is_err());
    assert!(m
        .rename_set("landscape/rock01", "landscape/rock01")
        .is_err());
    assert!(m.rename_set("landscape/rock01", "stone/boulder").is_ok());
    idle(&mut m);
    let v = set_views(&m);
    assert!(view(&v, "stone/boulder").pbr);
    assert!(!v.iter().any(|s| s.id == "landscape/rock01"));
    // The old name is still the engine's until the rename applies, so another set may not take it.
    assert!(m.rename_set("landscape/leaf", "landscape/rock01").is_err());
    // Renaming back is a plain rename that leaves no stale edit.
    assert!(m.rename_set("stone/boulder", "landscape/rock01").is_ok());
    idle(&mut m);
    assert!(view(&set_views(&m), "landscape/rock01").pbr);
    assert!(engine_inputs(&m)
        .0
        .iter()
        .all(|e| !matches!(e, engine::sets::SetEdit::Rename { .. })));
}

#[test]
fn delete_set_only_deletes_created_sets() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    assert!(!m.delete_set("landscape/rock01"));
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    assert!(m.delete_set(&id));
    idle(&mut m);
    assert!(!set_views(&m).iter().any(|s| s.id == id));
}

#[test]
fn slot_paths_are_checked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let set = "landscape/rock01";
    assert!(m
        .set_slot_command(set, 6, SlotCommand::Path("../x.dds".into()))
        .is_err());
    assert!(m
        .set_slot_command(set, 6, SlotCommand::Path("textures/x.png".into()))
        .is_err());
    assert!(m
        .set_slot_command(set, 6, SlotCommand::Path("textures/x.dds".into()))
        .is_ok());
    let e = |m: &sdc_gui::model::AppModel| {
        m.sets_doc
            .as_ref()
            .unwrap()
            .entries()
            .into_iter()
            .find(|e| e.id == set)
            .unwrap()
            .settings
    };
    assert_eq!(e(&m).get("slot6"), Some(&json!(r"textures\x.dds")));
    m.set_slot_command(set, 6, SlotCommand::Clear).unwrap();
    assert_eq!(e(&m).get("slot6"), Some(&json!("")));
    m.set_slot_command(set, 6, SlotCommand::FromSet).unwrap();
    assert_eq!(e(&m).get("slot6"), None);
}

#[test]
fn a_read_only_project_file_refuses_set_edits_but_sdc_toml_still_edits() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let root = m.root.clone().unwrap();
    std::fs::write(root.join("sdc-project.toml"), "[[sets]\nbroken").unwrap();
    m.reload_documents();
    idle(&mut m);
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    let odd = key_of(&m, "odd.png");
    let rough = key_of(&m, "rock01_roughness.png");
    let leaf = key_of(&m, "leaf.png");
    assert!(matches!(
        m.drop_on_set(&leaf, "landscape/rock01"),
        DropOutcome::Refused(_)
    ));
    assert!(matches!(
        m.drop_choice(&rough, "landscape/odd", DropChoice::Duplicate),
        DropOutcome::Refused(_)
    ));
    assert!(!root.join("landscape/odd_roughness.png").exists());
    assert_eq!(m.drop_new_set(&odd), None);
    assert!(m.rename_set("landscape/rock01", "x/y").is_err());
    assert!(m.remove_member("landscape/rock01", &rough).is_err());
    m.set_json_value(Some("landscape/rock01"), "specular_level", Some(json!(0.1)));
    assert!(!m.can_undo(), "no set edit was recorded");
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.can_undo(), "sdc.toml edits still work");
}
