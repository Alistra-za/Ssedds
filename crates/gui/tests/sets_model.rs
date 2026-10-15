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
    // The plan went back to the mirrored path.
    let diffuse = m.items.iter().find(|i| i.name == "rock01.png").unwrap();
    let out = diffuse
        .job
        .as_ref()
        .unwrap()
        .output
        .to_string_lossy()
        .replace('\\', "/");
    assert!(out.ends_with("export/landscape/rock01.dds"), "{out}");
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
    m.drop_replace(&leaf, "landscape/rock01", DropChoice::Move)
        .unwrap();
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
fn emissive_and_skin_sk_cannot_share_a_set() {
    assert!(slot_conflict(&[SlotKind::Emissive], SlotKind::SkinSk).is_some());
    assert!(slot_conflict(&[SlotKind::SkinSk], SlotKind::Emissive).is_some());
    assert!(slot_conflict(&[SlotKind::Diffuse, SlotKind::Normal], SlotKind::SkinSk).is_none());
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
            (SlotKind::SkinSk, 3, "sk"),
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
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.05)));
    assert!(!m.busy(), "a material value does not plan");
    m.set_json_value(
        "landscape/rock01",
        "match1",
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
        "landscape/rock01",
        "match1",
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
    m.set_json_value("landscape/rock01", "match1", None);
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
    assert!(m.drop_new_set(&odd).is_err());
    assert!(m.rename_set("landscape/rock01", "x/y").is_err());
    assert!(m.remove_member("landscape/rock01", &rough).is_err());
    assert!(m
        .drop_replace(&leaf, "landscape/rock01", DropChoice::Move)
        .is_err());
    assert!(!m.delete_set("landscape/rock01"));
    assert!(m
        .set_slot_command("landscape/rock01", 6, SlotCommand::Clear)
        .is_err());
    let order = m.sets_doc.as_ref().unwrap().order();
    m.reorder_set("landscape/rock01", 3);
    assert_eq!(m.sets_doc.as_ref().unwrap().order(), order);
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.1)));
    assert!(!m.can_undo(), "no set edit was recorded");
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.can_undo(), "sdc.toml edits still work");
}

/// rock01 (PBR) plus a created set `landscape/leaf-2` holding leaf.png in its diffuse slot.
fn with_leaf_set(d: &std::path::Path) -> (sdc_gui::model::AppModel, String) {
    let mut m = pbr_opened(d);
    idle(&mut m);
    let leaf = key_of(&m, "leaf.png");
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    assert!(matches!(
        view(&set_views(&m), &id).slots.get(&SlotKind::Diffuse),
        Some(SlotFill::Own { item }) if *item == leaf
    ));
    (m, id)
}

#[test]
fn sharing_into_a_filled_slot_asks_to_replace() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, id) = with_leaf_set(d.path());
    let leaf = key_of(&m, "leaf.png");
    let rock = key_of(&m, "rock01.png");
    let before = m.sets_doc.as_ref().unwrap().text();
    assert_eq!(
        m.drop_choice(&rock, &id, DropChoice::Share),
        DropOutcome::AskReplace {
            existing: leaf.clone()
        }
    );
    // Cancel: nothing changed.
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
    // Replace: leaf leaves, the slot borrows rock01's diffuse, in one step.
    m.drop_replace(&rock, &id, DropChoice::Share).unwrap();
    idle(&mut m);
    let v = set_views(&m);
    assert!(matches!(
        view(&v, &id).slots.get(&SlotKind::Diffuse),
        Some(SlotFill::Borrowed { from, item }) if from == "landscape/rock01" && *item == rock
    ));
    assert!(!view(&v, &id).members.contains(&leaf));
    m.undo();
    idle(&mut m);
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
}

#[test]
fn sharing_away_the_only_texture_of_an_automatic_set_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let rock = key_of(&m, "rock01.png");
    assert!(matches!(
        m.drop_choice(&rock, "landscape/leaf", DropChoice::Share),
        DropOutcome::AskReplace { .. }
    ));
    assert!(m
        .drop_replace(&rock, "landscape/leaf", DropChoice::Share)
        .is_err());
}

#[test]
fn duplicating_a_finished_map_into_a_filled_slot_asks_to_replace() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, id) = with_leaf_set(d.path());
    let leaf = key_of(&m, "leaf.png");
    let rock = key_of(&m, "rock01.png");
    let base = id.rsplit('/').next().unwrap().to_string();
    let copy = m
        .root
        .clone()
        .unwrap()
        .join(format!("landscape/{base}.png"));
    assert_eq!(
        m.drop_choice(&rock, &id, DropChoice::Duplicate),
        DropOutcome::AskReplace {
            existing: leaf.clone()
        }
    );
    assert!(!copy.exists(), "cancel copies nothing");
    m.drop_replace(&rock, &id, DropChoice::Duplicate).unwrap();
    assert!(copy.exists(), "{copy:?}");
    idle(&mut m);
    let members = view(&set_views(&m), &id).members.clone();
    assert!(
        members.contains(&format!("landscape/{base}.png")),
        "{members:?}"
    );
    assert!(!members.contains(&leaf));
    m.undo();
    assert!(
        !copy.exists(),
        "one undo removes the copy and puts leaf back"
    );
    idle(&mut m);
    assert!(view(&set_views(&m), &id).members.contains(&leaf));
}

#[test]
fn an_entry_whose_files_are_all_gone_shows_as_missing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.05)));
    let root = m.root.clone().unwrap();
    for f in [
        "rock01.png",
        "rock01_normal.png",
        "rock01_roughness.png",
        "rock01_ao.png",
    ] {
        std::fs::remove_file(root.join("landscape").join(f)).unwrap();
    }
    m.reload_changed();
    idle(&mut m);
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    assert!(rock.key.is_empty() && rock.members.is_empty());
    assert!(!rock.missing.is_empty(), "shown as missing");
    let leaf = key_of(&m, "leaf.png");
    assert!(matches!(
        m.drop_on_set(&leaf, "landscape/rock01"),
        DropOutcome::Refused(_)
    ));
}

#[test]
fn a_borrow_from_a_set_without_that_slot_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.edit_docs("t", None, true, |_, s| {
        s.set_borrow("landscape/leaf", 6, Some("landscape/odd"))
    });
    idle(&mut m);
    let v = set_views(&m);
    let leaf = view(&v, "landscape/leaf");
    assert!(
        leaf.missing
            .iter()
            .any(|x| x.contains("slot6") && x.contains("landscape/odd")),
        "{:?}",
        leaf.missing
    );
    assert!(!leaf.slots.contains_key(&SlotKind::Rmaos));
}

#[test]
fn export_refuses_a_plan_that_does_not_match_the_sets() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    // A set change that was not planned (the state the bounded re-planning leaves when it gives up).
    m.edit_docs("t", None, false, |_, s| s.create_set("x/y"));
    m.export_clicked();
    assert!(matches!(m.dialog, Some(sdc_gui::model::Dialog::Error(_))));
    assert!(m.export.running.is_none(), "nothing was exported");
}

#[test]
fn an_entry_left_with_only_its_id_is_not_an_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.rename_set("landscape/leaf", "landscape/leafy").unwrap();
    idle(&mut m);
    m.rename_set("landscape/leafy", "landscape/leaf").unwrap();
    idle(&mut m);
    let leaf = key_of(&m, "leaf.png");
    assert!(!matches!(
        m.drop_on_set(&leaf, "landscape/rock01"),
        DropOutcome::AskChoice { .. }
    ));
}

#[test]
fn the_entry_of_a_set_whose_files_are_gone_can_be_deleted() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value("landscape/leaf", "specular_level", Some(json!(0.05)));
    std::fs::remove_file(m.root.clone().unwrap().join("landscape/leaf.png")).unwrap();
    m.reload_changed();
    idle(&mut m);
    assert!(view(&set_views(&m), "landscape/leaf").key.is_empty());
    assert!(m.delete_set("landscape/leaf"));
    idle(&mut m);
    assert!(!set_views(&m).iter().any(|s| s.id == "landscape/leaf"));
}

/// Final review I1: the packed output's `[files]` key follows its first source's name, as the engine scopes it, so a
/// format chosen on the RMAOS card still applies after a match edit moves the output under another prefix.
#[test]
fn a_packed_format_applies_after_a_match_edit() {
    use engine::config::{ColorTag, Format};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let packed = |m: &sdc_gui::model::AppModel| {
        m.items
            .iter()
            .find(|i| i.name.ends_with("rmaos"))
            .unwrap()
            .clone()
    };
    let before = packed(&m).key;
    m.set_json_value(
        "landscape/rock01",
        "match1",
        Some(json!(r"landscape\stone")),
    );
    idle(&mut m);
    let it = packed(&m);
    assert!(
        it.job
            .as_ref()
            .unwrap()
            .output
            .to_string_lossy()
            .contains("stone_rmaos"),
        "the match edit moved the output: {:?}",
        it.job.as_ref().map(|j| &j.output)
    );
    assert_eq!(
        it.key, before,
        "the item key does not follow the output path"
    );
    let want = if it.job.as_ref().unwrap().profile.format == Format::Bc1 {
        Format::Bc7
    } else {
        Format::Bc1
    };
    m.set_format(&it.key, want, ColorTag::Unorm);
    idle(&mut m);
    assert_eq!(packed(&m).job.as_ref().unwrap().profile.format, want);
    assert!(m.unmatched_keys.is_empty(), "{:?}", m.unmatched_keys);
}

/// Final review I1: a packed `[files]` entry made before a match edit still names the packed output afterwards (the
/// engine's unmatched-key check uses the same first-source stem), so it is not offered for removal.
#[test]
fn a_packed_entry_stays_matched_after_a_match_edit() {
    use engine::config::{ColorTag, Format};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let key = key_of(&m, "rock01_rmaos");
    m.set_format(&key, Format::Bc1, ColorTag::Unorm);
    m.set_json_value(
        "landscape/rock01",
        "match1",
        Some(json!(r"landscape\stone")),
    );
    idle(&mut m);
    let it = m.items.iter().find(|i| i.key == key).unwrap();
    assert_eq!(it.job.as_ref().unwrap().profile.format, Format::Bc1);
    assert!(m.unmatched_keys.is_empty(), "{:?}", m.unmatched_keys);
}

/// Final review I3: set ids fold as the engine folds them (Unicode lower case), so a rename to a name with a
/// non-ASCII capital stays one set and keeps its settings.
#[test]
fn a_rename_with_a_non_ascii_capital_keeps_one_set_and_its_settings() {
    use sdc_gui::model::pbr_export::json_entries;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.2)));
    m.rename_set("landscape/rock01", "landscape/Öl").unwrap();
    idle(&mut m);
    let views = set_views(&m);
    let same: Vec<_> = views
        .iter()
        .filter(|v| v.id.to_lowercase() == "landscape/öl")
        .collect();
    assert_eq!(same.len(), 1, "{views:#?}");
    assert!(views.iter().all(|v| v.missing.is_empty()), "{views:#?}");
    assert!(same[0].pbr && !same[0].key.is_empty());
    assert_eq!(same[0].settings.get("specular_level"), Some(&json!(0.2)));
    let entries = json_entries(&m);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].1.settings.get("specular_level"),
        Some(&json!(0.2))
    );
}

/// Final review I3: a set created in a folder whose name has a non-ASCII capital is one set (created, no phantom).
#[test]
fn a_created_set_in_a_non_ascii_folder_is_one_set() {
    use image::{ImageBuffer, Rgb};
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let f = root.join("Äpfel");
    std::fs::create_dir_all(&f).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(f.join("stein.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(f.join("stein_n.png"))
        .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = sdc_gui::model::AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    idle(&mut m);
    let normal = key_of(&m, "stein_n.png");
    assert!(normal.starts_with("Äpfel/"), "{normal}");
    let id = m.drop_new_set(&normal).unwrap();
    idle(&mut m);
    let views = set_views(&m);
    let folded = id.to_lowercase();
    let same: Vec<_> = views
        .iter()
        .filter(|v| v.id.to_lowercase() == folded)
        .collect();
    assert_eq!(same.len(), 1, "{id}: {views:#?}");
    assert!(same[0].created && !same[0].key.is_empty(), "{:#?}", same[0]);
    assert!(same[0].members.contains(&normal));
    assert!(views.iter().all(|v| v.missing.is_empty()), "{views:#?}");
}

#[test]
fn a_shared_rmaos_is_sent_to_the_engine_for_owner_and_borrower() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    assert!(shared_roughness(&m).is_empty());
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
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
    let shared = shared_roughness(&m);
    assert!(shared.contains("landscape/rock01"), "{shared:?}");
    assert!(shared.contains(&id), "{shared:?}");
    // Plan (`replan`) and export (`start_export`) both build their `BatchRequest` through `request()`.
    assert_eq!(m.request().unwrap().shared_roughness, shared);
}

/// Final review I1 (DECISIONS Note R M4-final-1): `rock01_normal` shared into a set with its own `rock02_rmaos`. The
/// borrower bakes its roughness from the borrowed `_n` and reports no missing companion.
#[test]
fn a_shared_normal_bakes_the_borrowers_own_rmaos() {
    use engine::codes::Code;
    use engine::config::Subtype;
    use image::{ImageBuffer, Rgb, Rgba};
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 60u8]))
        .save(r.join("rock02.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([(x * 4) as u8, 0, 200, 255]))
        .save(r.join("rock02_rmaos.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    m.decide("landscape/rock02.png", Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    let rmaos02 = key_of(&m, "rock02_rmaos.png");
    let member = |m: &sdc_gui::model::AppModel| {
        m.item(&rmaos02)
            .and_then(|i| i.job.clone())
            .expect("rock02_rmaos has a job")
    };
    assert_eq!(member(&m).profile.subtype, Subtype::Rmaos);
    // Before the share, rock02 has no `_n`: the honest code.
    assert!(member(&m)
        .warnings
        .iter()
        .any(|w| w.code == Code::SetMissingCompanion));
    let normal = key_of(&m, "rock01_normal.png");
    assert_eq!(
        m.drop_on_set(&normal, "landscape/rock02"),
        DropOutcome::AskChoice {
            from: "landscape/rock01".into()
        }
    );
    assert_eq!(
        m.drop_choice(&normal, "landscape/rock02", DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    let borrowed = borrowed_normal(&m);
    assert_eq!(
        borrowed.get("landscape/rock02").map(String::as_str),
        Some("landscape/rock01"),
        "{borrowed:?}"
    );
    assert!(shared_roughness(&m).is_empty());
    assert_eq!(m.request().unwrap().borrowed_normal, borrowed);
    let j = member(&m);
    let b = j.bake.as_ref().expect("the borrower's own rmaos bakes");
    assert!(b.normal.ends_with("rock01_normal.png"), "{:?}", b.normal);
    assert!(
        !j.warnings
            .iter()
            .any(|w| w.code == Code::SetMissingCompanion),
        "{:?}",
        j.warnings
    );
}

/// Final review Minor 1: after a vanilla → cs switch the first cs plan is requested with the old target's (empty)
/// shared-roughness list. The settle check compares that list too, so the plan on screen is re-planned even when the
/// output paths did not change (here rock01 has a bad `rename`, so it has no output paths at all).
#[test]
fn a_target_switch_replans_when_only_the_shared_roughness_changed() {
    use engine::config::Target;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    m.drop_on_set(&rmaos, &id);
    assert_eq!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    );
    m.set_json_value("landscape/rock01", "rename", Some(json!("../outside")));
    idle(&mut m);
    let bake = |m: &sdc_gui::model::AppModel| {
        m.item(&rmaos)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.bake.is_some())
    };
    assert_eq!(bake(&m), Some(false), "a shared rmaos never bakes");
    m.set_target(Target::Vanilla);
    idle(&mut m);
    m.set_target(Target::Cs);
    idle(&mut m);
    assert!(!shared_roughness(&m).is_empty());
    assert_eq!(
        bake(&m),
        Some(false),
        "the plan on screen still bakes the shared rmaos"
    );
}

#[test]
fn shader_dropdown_lists_match_the_mode() {
    use engine::{config::Target, shader::Shader};
    assert_eq!(shader_options(Target::Cs).len(), 13);
    assert!(!shader_options(Target::Vanilla).contains(&Shader::Pbr));
    // ENB has complex grass, vanilla does not (ruling R1).
    assert!(!shader_options(Target::Vanilla).contains(&Shader::ComplexGrass));
    assert!(shader_options(Target::Cpm).contains(&Shader::ComplexGrass));
    assert_eq!(
        shader_options(Target::Cpm).len(),
        shader_options(Target::Vanilla).len() + 1
    );
}

#[test]
fn choosing_and_clearing_a_shader_round_trips_in_sdc_project_toml() {
    use engine::shader::{Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let v = set_views(&m);
    assert_eq!(
        view(&v, "landscape/leaf").shader.as_ref().unwrap().shader,
        Shader::Landscape
    );
    assert!(matches!(
        view(&v, "landscape/leaf")
            .shader
            .as_ref()
            .unwrap()
            .provenance,
        ShaderOrigin::Auto(_)
    ));
    m.set_shader("landscape/leaf", Some(Shader::Hair));
    idle(&mut m);
    let v = set_views(&m);
    let leaf = view(&v, "landscape/leaf");
    assert_eq!(
        (
            leaf.shader_choice,
            leaf.shader.as_ref().unwrap().provenance.clone()
        ),
        (Some(Shader::Hair), ShaderOrigin::Set)
    );
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .text()
        .contains("shader = \"hair\""));
    assert_eq!(set_shaders(&m).get("landscape/leaf"), Some(&Shader::Hair));
    m.set_shader("landscape/leaf", None);
    idle(&mut m);
    assert!(!m.sets_doc.as_ref().unwrap().text().contains("shader"));
    assert_eq!(view(&set_views(&m), "landscape/leaf").shader_choice, None);
}

#[test]
fn switching_mode_falls_back_and_back() {
    use engine::{
        codes::Code,
        config::Target,
        shader::{Shader, ShaderOrigin},
    };
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_shader("landscape/rock01", Some(Shader::Pbr));
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    let pick = rock.shader.as_ref().unwrap();
    assert_eq!(
        (pick.shader, &pick.provenance),
        (Shader::Default, &ShaderOrigin::Fallback)
    );
    assert!(rock
        .set_warnings
        .iter()
        .any(|w| w.code == Code::ShaderUnavailable));
    assert_eq!(
        rock.shader_choice,
        Some(Shader::Pbr),
        "the saved choice stays"
    );
    m.set_target(Target::Cs);
    idle(&mut m);
    assert_eq!(
        view(&set_views(&m), "landscape/rock01")
            .shader
            .as_ref()
            .unwrap()
            .shader,
        Shader::Pbr
    );
}

/// A scan database where `tex` (a `textures/…` path with `.dds`) is slot 0 of these shapes:
/// (lighting shader type, SLSF2, shader kind, derived copy, enabled origin).
fn scan_db(db: &std::path::Path, tex: &str, shapes: &[(u32, u32, i32, bool, bool)]) {
    let s = modscan::store::Store::open(db).unwrap();
    let c = s.conn();
    c.execute("INSERT INTO path(id, p) VALUES (1, ?1)", [tex])
        .unwrap();
    c.execute_batch(
        "INSERT INTO texset(id, n, t0) VALUES (1, 1, 1);
         INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0);",
    )
    .unwrap();
    for (i, (ty, slsf2, kind, derived, enabled)) in shapes.iter().enumerate() {
        let id = i as i64 + 2;
        c.execute(
            "INSERT INTO path(id, p) VALUES (?1, ?2)",
            rusqlite::params![id, format!("meshes/m{id}.nif")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (?1, 1, ?2, ?2, ?1, ?3)",
            rusqlite::params![id, format!("mod{id}"), enabled],
        )
        .unwrap();
        c.execute(
            "INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (?1, ?1, ?1, 0, ?2)",
            rusqlite::params![id, derived],
        )
        .unwrap();
        c.execute(
            "INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES (?1, 0, 'BSTriShape', ?2, ?3, ?4, 1)",
            rusqlite::params![id, kind, ty, slsf2],
        )
        .unwrap();
    }
}

#[test]
fn mesh_votes_count_enabled_source_lighting_shapes_only() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    scan_db(
        &db,
        "textures/a/wall.dds",
        &[
            (6, 0, 1, false, true),       // votes hair
            (6, 1 << 23, 1, false, true), // TruePBR: no vote
            (0, 0, 2, false, true),       // an effect shader: no vote
            (0, 0, 1, true, true),        // PGPatcher's output copy: no vote
            (0, 0, 1, false, false),      // a disabled mod: no vote
            (4, 0, 1, false, true),       // Face Tint: not one of the 12, no vote
        ],
    );
    let keys = ["textures/a/wall".to_string(), "textures/a/none".to_string()]
        .into_iter()
        .collect();
    let v = sdc_gui::model::meshes::mesh_votes(&db, &keys).unwrap();
    assert_eq!(v.get("textures/a/wall"), Some(&vec![Shader::Hair]));
    assert!(!v.contains_key("textures/a/none"));
}

#[test]
fn a_scan_database_picks_the_set_shader() {
    use engine::shader::{AutoRule, Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    // The key the GUI asks for, taken from the plan: the leaf diffuse's game folder and output stem.
    let j = m
        .items
        .iter()
        .filter_map(|i| i.job.as_ref())
        .find(|j| {
            j.set_key.ends_with("landscape/leaf")
                && j.output.file_stem().is_some_and(|s| s == "leaf")
        })
        .expect("the leaf diffuse job");
    let key = engine::shader::mesh_key(&j.game_path, "leaf");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db(&db, &format!("{key}.dds"), &[(6, 0, 1, false, true)]);
    m.scan_db = Some(db);
    m.refresh_mesh_votes(false);
    idle(&mut m);
    let leaf = view(&set_views(&m), "landscape/leaf")
        .shader
        .clone()
        .unwrap();
    assert_eq!(
        (leaf.shader, leaf.provenance),
        (Shader::Hair, ShaderOrigin::Auto(AutoRule::Mesh))
    );
    // No database: the suffix rules again (the leaf is under textures/landscape).
    m.scan_db = None;
    m.refresh_mesh_votes(false);
    idle(&mut m);
    assert_eq!(
        view(&set_views(&m), "landscape/leaf")
            .shader
            .as_ref()
            .unwrap()
            .shader,
        Shader::Landscape
    );
}

/// A scan database where `tex` is slot 0 of these lighting shapes:
/// (shader type, `alpha_flags`, `alpha_threshold`, SLSF2, derived copy). Every origin is enabled.
type AlphaShape = (u32, Option<i64>, Option<i64>, u32, bool);

fn scan_db_alpha(db: &std::path::Path, tex: &str, shapes: &[AlphaShape]) {
    let s = modscan::store::Store::open(db).unwrap();
    let c = s.conn();
    c.execute("INSERT INTO path(id, p) VALUES (1, ?1)", [tex])
        .unwrap();
    c.execute_batch(
        "INSERT INTO texset(id, n, t0) VALUES (1, 1, 1);
         INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0);",
    )
    .unwrap();
    for (i, (ty, flags, threshold, slsf2, derived)) in shapes.iter().enumerate() {
        let id = i as i64 + 2;
        c.execute(
            "INSERT INTO path(id, p) VALUES (?1, ?2)",
            rusqlite::params![id, format!("meshes/m{id}.nif")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (?1, 1, ?2, ?2, ?1, 1)",
            rusqlite::params![id, format!("mod{id}")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (?1, ?1, ?1, 0, ?2)",
            rusqlite::params![id, derived],
        )
        .unwrap();
        c.execute(
            "INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id, alpha_flags, alpha_threshold) VALUES (?1, 0, 'BSTriShape', 1, ?2, ?3, 1, ?4, ?5)",
            rusqlite::params![id, ty, slsf2, flags, threshold],
        )
        .unwrap();
    }
}

/// M6 Track E spec §4.1, §6 (Note U5): one entry per source lighting shape, from NiAlphaProperty bit 0 (blend) and bit
/// 9 (test). No property is no alpha. TruePBR shapes and PGPatcher's output copies are left out; a shader type that
/// is no vote (Face Tint, 4) still counts.
#[test]
fn mesh_alpha_reads_the_alpha_property_of_each_source_lighting_shape() {
    use engine::shader::MeshAlpha;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    scan_db_alpha(
        &db,
        "textures/a/wall.dds",
        &[
            (0, Some(1), Some(0), 0, false),
            (0, Some(1 << 9), Some(128), 0, false),
            (4, Some(1 | 1 << 9), Some(90), 0, false),
            (0, None, None, 0, false),
            (0, Some(1 << 9), Some(200), 1 << 23, false),
            (0, Some(1), Some(0), 0, true),
        ],
    );
    let keys = ["textures/a/wall".to_string(), "textures/a/none".to_string()]
        .into_iter()
        .collect();
    let v = sdc_gui::model::meshes::mesh_alpha(&db, &keys).unwrap();
    let mut got = v["textures/a/wall"].clone();
    got.sort_by_key(|a| (a.blend, a.test, a.threshold));
    let ma = |blend, test, threshold| MeshAlpha {
        blend,
        test,
        threshold,
    };
    assert_eq!(
        got,
        vec![
            ma(false, false, 0),
            ma(false, true, 128),
            ma(true, false, 0),
            ma(true, true, 90),
        ]
    );
    assert!(!v.contains_key("textures/a/none"));
}

/// M6 Track E spec §1 success, plan Review Focus 3 and 4: the scan's alpha evidence reaches the plan. A refresh that
/// changes only the alpha (a Face Tint shape is no shader vote) still reports a change, and a varying-alpha diffuse
/// is then no longer asked its alpha meaning.
#[test]
fn a_scan_database_decides_a_diffuse_alpha() {
    use engine::config::AlphaPolicy;
    use sdc_gui::model::items::Decision;
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/a");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(16, 16, |x, _| image::Rgba([90u8, 60, 30, (x * 16) as u8]))
        .save(t.join("glass.png"))
        .unwrap();
    let mut m = opened_as_the_app(d.path().join("textures"), d.path().join("export"));
    let asked = |m: &sdc_gui::model::AppModel| {
        m.item("a/glass.png")
            .unwrap()
            .decisions
            .contains(&Decision::AlphaMeaning)
    };
    assert!(asked(&m));
    let j = m.item("a/glass.png").unwrap().job.clone().unwrap();
    let key = engine::shader::mesh_key(&j.game_path, "glass");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db_alpha(
        &db,
        &format!("{key}.dds"),
        &[(4, Some(1), Some(0), 0, false)],
    );
    m.scan_db = Some(db);
    m.refresh_mesh_votes(false);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.mesh_votes.is_empty(), "Face Tint is no shader vote");
    let j = m.item("a/glass.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (
            j.profile.alpha.policy,
            j.provenance["alpha.policy"].to_string()
        ),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
    assert!(!asked(&m));
}

/// F1 spec Review Focus 1: match1 comes from the source, so a set rename does not move it off the mesh's path.
#[test]
fn a_renamed_set_still_matches_its_source_path() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::pgp::writer::entry_node;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.rename_set("landscape/rock01", "landscape/boulder")
        .unwrap();
    idle(&mut m);
    let (_, e) = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/boulder")
        .expect("the renamed set is a PBR set");
    assert_eq!(
        entry_node(&e).to_value()["match1"],
        json!(r"landscape\rock01.dds")
    );
}

/// F1 spec §3.2, owner 2026-10-12: the Matching rows are the active matches and the empty + rows; the + menu offers
/// slots 3–8 without a row, a filled slot starting from the set's path and any other empty.
#[test]
fn the_matching_rows_and_the_plus_menu_follow_the_filled_slots() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let (active, add) = slot_matches(view(&set_views(&m), "landscape/rock01"));
    assert_eq!(
        active,
        vec![SlotMatch {
            n: 1,
            slot: "Diffuse".into(),
            path: r"landscape\rock01".into()
        }]
    );
    let slot = |n: u8, slot: &str, path: &str| SlotMatch {
        n,
        slot: slot.into(),
        path: path.into(),
    };
    assert_eq!(
        add,
        vec![
            slot(3, "Slot 3", ""),
            slot(4, "Slot 4", ""),
            slot(5, "Slot 5", ""),
            slot(6, "RMAOS", r"landscape\rock01_rmaos"),
            slot(7, "Slot 7", ""),
            slot(8, "Slot 8", ""),
        ]
    );
    m.set_json_value(
        "landscape/rock01",
        "match6",
        Some(json!(r"landscape\rock01_rmaos")),
    );
    m.set_json_value("landscape/rock01", "match3", Some(json!("")));
    m.set_json_value("landscape/rock01", "match1", Some(json!("")));
    idle(&mut m);
    let (active, add) = slot_matches(view(&set_views(&m), "landscape/rock01"));
    assert_eq!(
        active,
        vec![
            slot(3, "Slot 3", ""),
            slot(6, "RMAOS", r"landscape\rock01_rmaos")
        ],
        "an empty + row stays a row; match1 = \"\" (diffuse off) is not one"
    );
    assert_eq!(
        add.iter().map(|a| a.n).collect::<Vec<_>>(),
        vec![4, 5, 7, 8]
    );
}

/// F1 spec §5, DECISIONS V8: a PBR set's cards are the PBR slots in PGPatcher order 1–8, empty or not.
#[test]
fn a_pbr_set_shows_its_pbr_slots_in_slot_order() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let v = view(&set_views(&m), "landscape/rock01").clone();
    assert_eq!(v.shader.as_ref().map(|p| p.shader), Some(Shader::Pbr));
    let row = slot_row(v.shader.as_ref(), &v.slots);
    assert_eq!(
        row.iter().map(|c| c.kind).collect::<Vec<_>>(),
        vec![
            SlotKind::Diffuse,
            SlotKind::Normal,
            SlotKind::Emissive,
            SlotKind::Height,
            SlotKind::Rmaos,
            SlotKind::Coat,
            SlotKind::Fuzz,
            SlotKind::Subsurface,
        ]
    );
    assert!(row.iter().all(|c| c.usable));
    let set = |k: &[SlotKind]| k.iter().copied().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        usable_slots(Shader::AdvancedSkin),
        set(&[
            SlotKind::Diffuse,
            SlotKind::Normal,
            SlotKind::SkinSk,
            SlotKind::Rmaos
        ])
    );
    assert_eq!(
        usable_slots(Shader::Glow),
        set(&[SlotKind::Diffuse, SlotKind::Normal, SlotKind::Emissive])
    );
    assert_eq!(
        usable_slots(Shader::Eye),
        set(&[SlotKind::Diffuse, SlotKind::Normal])
    );
}

/// F1 spec §5, Review Focus 4: a SHADER-UNAVAILABLE fallback (default in place of pbr) keeps the filled RMAOS
/// visible, in slot order and marked; with no pick, only the filled slots show.
#[test]
fn a_fallback_shader_keeps_filled_slots_it_cannot_use() {
    use engine::shader::{Shader, ShaderOrigin, ShaderPick};
    use std::collections::BTreeMap;
    let own = |i: &str| SlotFill::Own { item: i.into() };
    let slots: BTreeMap<SlotKind, SlotFill> =
        [(SlotKind::Rmaos, own("r")), (SlotKind::Diffuse, own("d"))]
            .into_iter()
            .collect();
    let fallback = ShaderPick {
        shader: Shader::Default,
        provenance: ShaderOrigin::Fallback,
        wanted: Some(Shader::Pbr),
    };
    let row = slot_row(Some(&fallback), &slots);
    assert_eq!(
        row,
        vec![
            SlotCell {
                kind: SlotKind::Diffuse,
                fill: own("d"),
                usable: true
            },
            SlotCell {
                kind: SlotKind::Normal,
                fill: SlotFill::Empty,
                usable: true
            },
            SlotCell {
                kind: SlotKind::Rmaos,
                fill: own("r"),
                usable: false
            },
        ]
    );
    let none = slot_row(None, &slots);
    assert_eq!(
        none.iter().map(|c| (c.kind, c.usable)).collect::<Vec<_>>(),
        vec![(SlotKind::Diffuse, true), (SlotKind::Rmaos, true)]
    );
}

/// Controller ruling (final fix wave): + prefills slot N only when PGPatcher's slot-N list strips the member's
/// suffix, so a prefilled row never splits the prefix. Subsurface `_s` in slot 8 is not in that list (`_b`/`_bl`/
/// `_flow`): it gets an empty row instead. Height `_p` in slot 4 is stripped, so it is prefilled.
#[test]
fn the_plus_menu_prefills_only_suffixes_pgpatcher_strips() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let mut v = view(&set_views(&m), "landscape/rock01").clone();
    for k in [SlotKind::Height, SlotKind::Subsurface] {
        v.slots.insert(
            k,
            SlotFill::Own {
                item: "landscape/x.png".into(),
            },
        );
    }
    let (_, add) = slot_matches(&v);
    let path = |n: u8| add.iter().find(|a| a.n == n).map(|a| a.path.clone());
    assert_eq!(path(4), Some(r"landscape\rock01_p".into()));
    assert_eq!(path(6), Some(r"landscape\rock01_rmaos".into()));
    assert_eq!(path(8), Some(String::new()), "slot 8 does not strip _s");
}

#[test]
fn the_scan_database_is_read_off_the_ui_thread_and_a_read_error_keeps_the_values() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let j = m
        .items
        .iter()
        .filter_map(|i| i.job.as_ref())
        .find(|j| {
            j.set_key.ends_with("landscape/leaf")
                && j.output.file_stem().is_some_and(|s| s == "leaf")
        })
        .expect("the leaf diffuse job");
    let key = engine::shader::mesh_key(&j.game_path, "leaf");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db(&db, &format!("{key}.dds"), &[(6, 0, 1, false, true)]);
    m.scan_db = Some(db.clone());
    m.refresh_mesh_votes(false);
    // The read is on a worker: nothing is applied until the model polls, and the model reports busy meanwhile.
    assert!(m.mesh_votes.is_empty() && m.busy());
    idle(&mut m);
    assert!(!m.mesh_votes.is_empty(), "the result landed in poll");
    // A database that cannot be read keeps the previous evidence (and logs to stderr), it does not clear it.
    let before = m.mesh_votes.clone();
    std::fs::write(&db, b"not a database").unwrap();
    m.refresh_mesh_votes(false);
    idle(&mut m);
    assert_eq!(m.mesh_votes, before);
}

/// A model with a leaf mesh vote in a scan database, planned again until its post-plan read is pending.
fn pending_read(d: &std::path::Path) -> (sdc_gui::model::AppModel, tempfile::TempDir) {
    let mut m = pbr_opened(d);
    idle(&mut m);
    let j = m
        .items
        .iter()
        .filter_map(|i| i.job.as_ref())
        .find(|j| {
            j.set_key.ends_with("landscape/leaf")
                && j.output.file_stem().is_some_and(|s| s == "leaf")
        })
        .expect("the leaf diffuse job");
    let key = engine::shader::mesh_key(&j.game_path, "leaf");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db(&db, &format!("{key}.dds"), &[(6, 0, 1, false, true)]);
    m.scan_db = Some(db);
    m.replan();
    let end = std::time::Instant::now() + Duration::from_secs(60);
    while !m.votes_pending() {
        assert!(std::time::Instant::now() < end, "the plan never landed");
        m.poll();
    }
    assert!(m.mesh_votes.is_empty());
    (m, dbdir)
}

fn export_started_or_asked(m: &sdc_gui::model::AppModel) -> bool {
    m.dialog.is_some() || m.export.running.is_some() || m.export.summary.is_some()
}

#[test]
fn a_queued_export_fires_once_the_post_plan_read_lands() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    m.export_clicked();
    assert!(!export_started_or_asked(&m), "waits for the read");
    idle(&mut m);
    assert!(!m.mesh_votes.is_empty());
    assert!(export_started_or_asked(&m), "the tail of the plan ran");
}

#[test]
fn a_scan_finished_refresh_keeps_the_pending_plan_continuation() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    m.export_clicked();
    m.refresh_mesh_votes(false);
    idle(&mut m);
    assert!(export_started_or_asked(&m), "the plan still finished");
}

#[test]
fn an_edit_during_a_pending_read_drops_the_stale_read() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    m.export_clicked();
    m.replan();
    assert!(!m.votes_pending(), "the stale read was cleared");
    m.poll();
    assert!(m.mesh_votes.is_empty(), "its evidence is never applied");
    assert!(!export_started_or_asked(&m), "no finish on the old plan");
    idle(&mut m);
    assert!(!m.mesh_votes.is_empty(), "the new plan read again");
    assert!(export_started_or_asked(&m));
}

#[test]
fn the_json_panel_and_its_errors_belong_to_json_sets_only() {
    use engine::config::Target;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert_eq!(m.target, Target::Cs);
    let plain = view(&set_views(&m), "landscape/rock01").clone();
    assert!(!plain.pbr && !plain.skin);
    assert!(!shows_json_panel(&plain, Target::Cs));
    // Existing JSON settings are never hidden.
    let mut with_json = plain.clone();
    with_json.settings.insert("slot1".into(), json!("x"));
    assert!(shows_json_panel(&with_json, Target::Cs));
    // A PBR set shows it on cs only.
    let d = tempfile::tempdir().unwrap();
    m = pbr_opened(d.path());
    let pbr = view(&set_views(&m), "landscape/rock01").clone();
    assert!(pbr.pbr && shows_json_panel(&pbr, Target::Cs));
    assert!(!shows_json_panel(&pbr, Target::Vanilla));
}

#[test]
fn a_grass_diffuse_card_is_titled_complex_grass() {
    use engine::shader::Shader;
    assert_eq!(
        card_title(SlotKind::Diffuse, "a.png", Some(Shader::ComplexGrass)),
        "Complex grass"
    );
    assert_eq!(
        card_title(SlotKind::Diffuse, "a.png", Some(Shader::Landscape)),
        "Diffuse"
    );
    assert_eq!(
        card_title(SlotKind::Normal, "a.png", Some(Shader::ComplexGrass)),
        "Normal _n"
    );
}

#[test]
fn a_bad_prefix_on_a_non_json_set_is_no_export_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_json_value("landscape/rock01", "rename", Some(json!("a/b")));
    idle(&mut m);
    let v = view(&set_views(&m), "landscape/rock01").clone();
    assert!(!v.pbr && v.prefix_error.is_some(), "{v:?}");
    assert!(sdc_gui::model::pbr_export::export_blocks(&m)
        .iter()
        .all(|b| !b.text.contains("rename")));
}

/// Round 3 review: the settings Format row writes the tag with the format, so its reset clears both.
#[test]
fn resetting_format_also_clears_the_tag_it_wrote() {
    use engine::config::{ColorTag, Format};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let key = key_of(&m, "rock01_rmaos");
    m.set_format(&key, Format::Bc1, ColorTag::Unorm);
    idle(&mut m);
    let p = m.project.as_ref().unwrap();
    assert!(p.file_value(&key, "format").is_some() && p.file_value(&key, "color.tag").is_some());
    m.clear_field(&key, "format");
    idle(&mut m);
    let p = m.project.as_ref().unwrap();
    assert!(p.file_value(&key, "format").is_none());
    assert!(
        p.file_value(&key, "color.tag").is_none(),
        "tag outlived the format reset"
    );
}

/// Audit 2026-10-13b E-I1 (DECISIONS AF4): Export clicked during a scan-finished read (not a plan's own) starts
/// when that read lands, with unchanged evidence and with a read error; it never waits for an unrelated plan.
#[test]
fn an_export_clicked_during_a_scan_finished_read_starts_when_it_lands() {
    for broken in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let (mut m, db) = pending_read(d.path());
        idle(&mut m);
        assert!(!m.mesh_votes.is_empty());
        if broken {
            let bad = db.path().join("broken.db");
            std::fs::write(&bad, b"not a database").unwrap();
            m.scan_db = Some(bad);
        }
        m.refresh_mesh_votes(false);
        assert!(m.votes_pending(), "broken={broken}");
        m.export_clicked();
        assert!(!export_started_or_asked(&m), "waits for the read");
        idle(&mut m);
        assert!(
            export_started_or_asked(&m),
            "broken={broken}: started when the read landed"
        );
    }
}

/// Review round 1 M5 (DECISIONS AF4): an edit that supersedes the scan-finished read the export was waiting for
/// drops the queued export; the edit's plan never starts it unasked.
#[test]
fn an_edit_during_a_scan_finished_read_drops_the_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    idle(&mut m);
    m.refresh_mesh_votes(false);
    m.export_clicked();
    m.set_field("landscape/rock01.png", "header", json!("dx10"));
    idle(&mut m);
    assert!(!export_started_or_asked(&m), "{:?}", m.dialog);
    assert!(
        m.status_line
            .contains("Export cancelled: the project changed while scan results were loading"),
        "{}",
        m.status_line
    );
    assert!(m
        .messages
        .entries()
        .any(|e| e.text.starts_with("Export cancelled: the project changed")));
}

/// Fix round 2 (DECISIONS AF4): a re-plan that is no edit (here an NVTT probe whose result changed) keeps an Export
/// queued behind a scan-finished read; that plan carries it to its end.
#[test]
fn a_non_edit_replan_during_a_scan_finished_read_keeps_the_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    idle(&mut m);
    m.nvtt = Some(engine::nvtt::NvttProbe::Missing("before".into()));
    // A probe result that differs (generation 0: this model never probed) re-plans. It is queued before the read
    // starts, so the channel (first in, first out) hands it over first: the re-plan runs while the read is pending.
    m.sender()
        .send(sdc_gui::worker::WorkerMsg::Nvtt {
            generation: 0,
            probe: engine::nvtt::NvttProbe::Missing("after".into()),
        })
        .unwrap();
    m.refresh_mesh_votes(false);
    m.export_clicked();
    assert!(m.votes_pending() && !export_started_or_asked(&m));
    idle(&mut m);
    assert!(
        export_started_or_asked(&m),
        "the probe's re-plan kept the export"
    );
}

/// Fix round 3 (DECISIONS AF11): an undo changes the project like an edit, so it cancels an Export queued behind a
/// scan-finished read, with the note.
#[test]
fn an_undo_during_a_scan_finished_read_cancels_the_queued_export() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _db) = pending_read(d.path());
    idle(&mut m);
    m.set_field("landscape/rock01.png", "header", json!("dx10"));
    idle(&mut m);
    m.refresh_mesh_votes(false);
    m.export_clicked();
    m.undo();
    idle(&mut m);
    assert!(!export_started_or_asked(&m), "{:?}", m.dialog);
    assert!(
        m.status_line
            .contains("Export cancelled: the project changed while scan results were loading"),
        "{}",
        m.status_line
    );
}
