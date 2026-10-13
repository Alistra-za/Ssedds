//! F3a Task 8 (ledger R8): a per-slot lock keeps a set's slot as it is.
mod support;
use sdc_gui::model::bulk::{bulk_fields, plan_bulk_edit, BulkOp, Change};
use sdc_gui::model::sets::*;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;
use support::*;

const SET: &str = "landscape/rock01";
const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const ODD: &str = "landscape/odd.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn strings(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

fn locks(m: &AppModel) -> Vec<u8> {
    set_views(m)
        .into_iter()
        .find(|v| v.id == SET)
        .unwrap()
        .locks
        .into_iter()
        .collect()
}

#[test]
fn a_locked_slot_refuses_drops_onto_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_slot_lock(SET, 1, true).unwrap();
    assert!(m.slot_locked(SET, 1));
    assert_eq!(locks(&m), vec![1]);
    let refused = |o: DropOutcome| matches!(o, DropOutcome::Refused(why) if why.contains("slot 1 of landscape/rock01 is locked"));
    assert!(refused(m.drop_on_set(LEAF, SET)));
    assert!(refused(m.drop_choice(LEAF, SET, DropChoice::Move)));
    assert!(m.drop_replace(LEAF, SET, DropChoice::Move).is_err());
    // The locked fill cannot leave either.
    assert!(m.remove_member(SET, ROCK).is_err());
    // The new-set area says why (the UI shows it as the refusal note).
    let why = m.drop_new_set(ROCK).unwrap_err();
    assert!(
        why.contains("slot 1 of landscape/rock01 is locked"),
        "{why}"
    );
    // Another slot of the set is not locked.
    assert!(!m.slot_locked(SET, 2));
}

#[test]
fn a_locked_slot_refuses_slot_commands_until_unlocked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_slot_lock(SET, 6, true).unwrap();
    assert_eq!(
        m.set_slot_command(SET, 6, SlotCommand::Clear),
        Err("slot locked".to_string())
    );
    m.set_slot_lock(SET, 6, false).unwrap();
    assert_eq!(m.set_slot_command(SET, 6, SlotCommand::Clear), Ok(()));
}

#[test]
fn bulk_edits_skip_the_texture_filling_a_locked_slot() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let ks = strings(&[ROCK, ODD]);
    m.set_slot_lock(SET, 1, true).unwrap();
    let locked = Change::Skip("slot locked".into());
    for op in [BulkOp::Set(json!("veryfast")), BulkOp::Clear] {
        assert_eq!(
            plan_bulk_edit(&m, &ks, "encoder.effort", &op),
            vec![
                (ROCK.to_string(), locked.clone()),
                (ODD.to_string(), Change::Write)
            ]
        );
    }
    let row = bulk_fields(&m, &ks)
        .into_iter()
        .find(|f| f.field == "encoder.effort")
        .unwrap();
    assert_eq!(row.applies, 1);
    assert_eq!(row.skipped, vec![("slot locked".to_string(), 1)]);
    // A replan and a save and reload keep the lock.
    m.replan();
    idle(&mut m);
    assert_eq!(
        plan_bulk_edit(&m, &ks, "encoder.effort", &BulkOp::Clear)[0].1,
        locked
    );
    m.save().unwrap();
    m.reload_documents();
    idle(&mut m);
    assert!(m.slot_locked(SET, 1));
    assert!(
        std::fs::read_to_string(m.root.clone().unwrap().join("sdc-project.toml"))
            .unwrap()
            .contains("lock = [\"slot1\"]")
    );
    assert_eq!(
        plan_bulk_edit(&m, &ks, "encoder.effort", &BulkOp::Clear)[0].1,
        locked
    );
    // Not a PGPatcher setting.
    let v = set_views(&m).into_iter().find(|v| v.id == SET).unwrap();
    assert!(!v.settings.contains_key("lock"));
}

#[test]
fn lock_and_unlock_are_one_undo_step_each() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_slot_lock(SET, 2, true).unwrap();
    m.set_slot_lock(SET, 2, false).unwrap();
    assert!(!m.slot_locked(SET, 2));
    m.undo();
    idle(&mut m);
    assert!(m.slot_locked(SET, 2));
    m.undo();
    idle(&mut m);
    assert!(!m.slot_locked(SET, 2));
    m.redo();
    idle(&mut m);
    assert!(m.slot_locked(SET, 2));
}

#[test]
fn two_slot_commands_on_one_slot_are_two_undo_steps() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_slot_command(SET, 6, SlotCommand::Clear).unwrap();
    m.set_slot_command(SET, 6, SlotCommand::Path(r"textures\x.dds".into()))
        .unwrap();
    m.undo();
    idle(&mut m);
    let slot6 = |m: &AppModel| {
        set_views(m)
            .into_iter()
            .find(|v| v.id == SET)
            .unwrap()
            .settings
            .get("slot6")
            .cloned()
    };
    assert_eq!(slot6(&m), Some(json!("")));
}

/// A vanilla `_n` beside a diffuse in `m/` (the `pbr_members` fixture, smaller): it follows a PBR diffuse.
fn follow_fixture(dir: &std::path::Path) -> AppModel {
    use image::{ImageBuffer, Rgb};
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(r.join("rock01.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("rock01_n.png"))
        .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(dir.join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

#[test]
fn auto_retyping_leaves_a_locked_slot_alone() {
    use engine::config::Subtype;
    use sdc_gui::model::Answer;
    const N: &str = "m/rock01_n.png";
    let subtype = |m: &AppModel| m.item(N).unwrap().job.as_ref().unwrap().profile.subtype;
    // Unlocked, the inferred normal follows a PBR diffuse.
    let d = tempfile::tempdir().unwrap();
    let mut m = follow_fixture(d.path());
    m.decide("m/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m), Subtype::PbrNormal);
    // Locked, it does not.
    let d = tempfile::tempdir().unwrap();
    let mut m = follow_fixture(d.path());
    m.set_slot_lock("m/rock01", 2, true).unwrap();
    let before = subtype(&m);
    assert_ne!(before, Subtype::PbrNormal);
    m.decide("m/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m), before);
}

#[test]
fn a_locked_slot_that_borrows_a_texture_names_its_set_in_the_skip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    let id = m.drop_new_set(ODD).unwrap();
    idle(&mut m);
    assert_eq!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    m.set_slot_lock(&id, 6, true).unwrap();
    assert_eq!(
        plan_bulk_edit(
            &m,
            std::slice::from_ref(&rmaos),
            "encoder.effort",
            &BulkOp::Clear
        )[0]
        .1,
        Change::Skip(format!("slot locked in {id}"))
    );
    // Its own set's lock wins: plain "slot locked".
    m.set_slot_lock(SET, 6, true).unwrap();
    assert_eq!(
        plan_bulk_edit(&m, &[rmaos], "encoder.effort", &BulkOp::Clear)[0].1,
        Change::Skip("slot locked".into())
    );
}

#[test]
fn reset_all_skips_a_texture_filling_a_locked_slot() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    for k in [ROCK, ODD] {
        m.set_field(k, "encoder.effort", json!("veryfast"));
    }
    idle(&mut m);
    m.set_slot_lock(SET, 1, true).unwrap();
    m.reset_all_settings(&strings(&[ROCK, ODD]));
    idle(&mut m);
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.file_value(ROCK, "encoder.effort"),
        Some(json!("veryfast"))
    );
    assert_eq!(p.file_value(ROCK, "subtype"), Some(json!("pbr_albedo")));
    assert_eq!(p.file_value(ODD, "encoder.effort"), None);
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Reset all settings on 1 texture; locked on 1 of 2: slot locked")
    );
}

#[test]
fn auto_retyping_leaves_a_texture_borrowed_into_a_locked_slot_alone() {
    use engine::config::Subtype;
    use image::{ImageBuffer, Rgb};
    use sdc_gui::model::Answer;
    const N: &str = "m/rock01_n.png";
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, 7u8, (y * 4) as u8]))
        .save(d.path().join("m/leaf.png"))
        .unwrap();
    let mut m = follow_fixture(d.path());
    assert_eq!(
        m.drop_choice(N, "m/leaf", DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    m.set_slot_lock("m/leaf", 2, true).unwrap();
    let before = m.item(N).unwrap().job.as_ref().unwrap().profile.subtype;
    assert_ne!(before, Subtype::PbrNormal);
    m.decide("m/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(
        m.item(N).unwrap().job.as_ref().unwrap().profile.subtype,
        before
    );
}
