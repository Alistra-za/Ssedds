//! Note AN (owner, 2026-10-15): a PGPatcher entry carries one match key (a renamed set matches by `match_diffuse`
//! only), and the slot menu's "Keep original texture" toggles the slot's PGPatcher `lock_<slot>` key.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::bulk::MenuAction;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::sets::{set_views, SlotKind};
use sdc_gui::model::AppModel;
use sdc_gui::pgp::prefix::{
    extra_match_refusal, lock_key, pitfalls, RENAMED_MATCH_NOTE, RENAMED_ONE_MATCH,
};
use sdc_gui::pgp::settings::Settings;
use sdc_gui::pgp::writer::{document, EntryInput};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use support::*;

const SET: &str = "landscape/rock01";
const KEEP: &str = "Keep original texture (PGPatcher lock)";
const STOP: &str = "Stop keeping original texture (PGPatcher lock)";
const CAPTION: &str = "The original texture in this spot is used";
const HOVER: &str = "Kept by PGPatcher (lock_rmaos): the mesh's original texture is used here";

fn s(pairs: &[(&str, Value)]) -> Settings {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn written(pairs: &[(&str, Value)], slots: &[(u8, &str)]) -> Value {
    let e = EntryInput {
        settings: s(pairs),
        slots: slots
            .iter()
            .map(|(n, p)| (*n, p.to_string()))
            .collect::<BTreeMap<_, _>>(),
        derived_match: r"landscape\rock01".into(),
        section: false,
    };
    document(std::slice::from_ref(&e)).to_value()["entries"][0].clone()
}

#[test]
fn a_renamed_entry_writes_only_match_diffuse_and_rename() {
    assert_eq!(
        written(&[("rename", json!("boulder"))], &[]),
        json!({ "match_diffuse": r"landscape\rock01", "rename": r"landscape\boulder" })
    );
}

#[test]
fn an_entry_without_a_rename_writes_only_match1() {
    assert_eq!(
        written(&[], &[]),
        json!({ "match1": r"landscape\rock01.dds" })
    );
}

/// An old project stored `match1` and an extra `match2` next to the rename: it still exports one match key.
#[test]
fn an_old_renamed_project_with_stored_matches_exports_one_match_key() {
    let pairs = [
        ("match1", json!(r"landscape\rock01")),
        ("match2", json!(r"landscape\rock01_n")),
        ("rename", json!("boulder")),
    ];
    let v = written(&pairs, &[]);
    assert_eq!(
        v,
        json!({ "match_diffuse": r"landscape\rock01", "rename": r"landscape\boulder" })
    );
    // The extra match is reported as not written.
    let w = pitfalls(&s(&pairs), r"landscape\rock01");
    assert!(
        w.iter()
            .any(|m| m.contains("not written") && m.contains("match_diffuse")),
        "{w:?}"
    );
}

#[test]
fn a_renamed_set_refuses_an_extra_match() {
    let d = r"landscape\rock01";
    let renamed = s(&[("rename", json!("boulder"))]);
    let refuse =
        |st: &Settings, k: &str, v: Option<Value>| extra_match_refusal(st, d, k, v.as_ref());
    assert_eq!(
        refuse(&renamed, "match2", Some(json!(r"landscape\rock01_n"))),
        Some(RENAMED_ONE_MATCH)
    );
    // A new empty + row is an extra match too.
    assert_eq!(
        refuse(&renamed, "match3", Some(json!(""))),
        Some(RENAMED_ONE_MATCH)
    );
    // match1 back on (key removed) next to an active match2.
    let off = s(&[
        ("match1", json!("")),
        ("match2", json!(r"landscape\rock01_n")),
        ("rename", json!("boulder")),
    ]);
    assert_eq!(refuse(&off, "match1", None), Some(RENAMED_ONE_MATCH));
    // Editing the one match, removing it, or any match without a rename is fine.
    assert_eq!(
        refuse(&renamed, "match1", Some(json!(r"landscape\other"))),
        None
    );
    assert_eq!(refuse(&renamed, "match1", Some(json!(""))), None);
    assert_eq!(refuse(&s(&[]), "match2", Some(json!("x_n"))), None);
    assert!(RENAMED_ONE_MATCH.starts_with(RENAMED_MATCH_NOTE));
}

fn settings_of(m: &AppModel) -> Settings {
    set_views(m)
        .into_iter()
        .find(|v| v.id == SET)
        .unwrap()
        .settings
}

#[test]
fn the_model_refuses_an_extra_match_on_a_renamed_set_and_exports_one_key() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(SET, "rename", Some(json!("boulder")));
    m.set_json_value(SET, "match2", Some(json!(r"landscape\rock01_n")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!settings_of(&m).contains_key("match2"), "refused");
    let entries = json_entries(&m);
    let (_, e) = entries.iter().find(|(v, _)| v.id == SET).unwrap();
    let v = document(std::slice::from_ref(e)).to_value()["entries"][0].clone();
    assert!(
        v.get("match1").is_none() && v.get("match_diffuse").is_some(),
        "{v}"
    );
}

#[test]
fn lock_keys_map_by_slot_and_the_cubemap_has_none() {
    let keys: Vec<_> = (1..=8).map(lock_key).collect();
    assert_eq!(
        keys,
        vec![
            Some("lock_diffuse"),
            Some("lock_normal"),
            Some("lock_emissive"),
            Some("lock_parallax"),
            None,
            Some("lock_rmaos"),
            Some("lock_cnr"),
            Some("lock_subsurface"),
        ]
    );
}

#[test]
fn the_writer_emits_a_lock_and_drops_the_locked_slots_derived_path() {
    let v = written(
        &[("lock_normal", json!(true))],
        &[(2, r"textures\pbr\other\x_n.dds")],
    );
    assert_eq!(v["lock_normal"], json!(true));
    assert!(v.get("slot2").is_none(), "{v}");
}

#[test]
fn a_slot_command_next_to_its_lock_is_a_pitfall() {
    let w = pitfalls(
        &s(&[
            ("lock_normal", json!(true)),
            ("slot2", json!(r"textures\a_n.dds")),
        ]),
        r"landscape\rock01",
    );
    assert!(
        w.iter()
            .any(|m| m == "slot2 overrides the lock (PGPatcher applies slotN last)"),
        "{w:?}"
    );
    assert!(pitfalls(&s(&[("lock_normal", json!(true))]), r"landscape\rock01").is_empty());
}

#[test]
fn the_slot_menu_toggles_the_pgpatcher_lock_in_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let menu = m.slot_menu(SET, SlotKind::Normal);
    let keep = menu.iter().find(|e| e.label == KEEP).expect("menu entry");
    assert_eq!(
        keep.action,
        MenuAction::PgpLock {
            set: SET.into(),
            n: 2,
            on: true
        }
    );
    assert_eq!(keep.disabled, None);
    assert_eq!(m.run_menu(keep.action.clone()), None);
    assert_eq!(settings_of(&m).get("lock_normal"), Some(&json!(true)));
    assert!(m
        .slot_menu(SET, SlotKind::Normal)
        .iter()
        .any(|e| e.label == STOP));
    m.undo();
    assert!(
        !settings_of(&m).contains_key("lock_normal"),
        "one undo step"
    );
    // Off removes the key.
    m.redo();
    m.run_menu(MenuAction::PgpLock {
        set: SET.into(),
        n: 2,
        on: false,
    });
    assert!(!settings_of(&m).contains_key("lock_normal"));
}

#[test]
fn the_cubemap_lock_is_labelled_as_the_clear_it_is() {
    // No card has slot 5, so no menu offers it; the Texture locks group names what lock_cubemap does.
    let k = sdc_gui::pgp::schema::spec("lock_cubemap").unwrap();
    assert_eq!(k.label, "Clear cubemap");
    assert!(!SlotKind::ALL.iter().any(|k| k.slot_number() == 5));
}

#[test]
fn the_sdc_pin_is_renamed_and_still_works() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let menu = m.slot_menu(SET, SlotKind::Normal);
    let pin = menu
        .iter()
        .find(|e| e.label == "Pin slot (sdc)")
        .expect("pin entry");
    assert_eq!(m.run_menu(pin.action.clone()), None);
    assert!(m.slot_locked(SET, 2));
    assert!(m
        .slot_menu(SET, SlotKind::Normal)
        .iter()
        .any(|e| e.label == "Unpin slot (sdc)"));
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1600.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn a_locked_slot_card_shows_the_cross_and_one_tooltip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    // The RMAOS card is filled (the packed roughness + AO), so it has a path hover of its own to hide.
    let rmaos = match set_views(&m)
        .into_iter()
        .find(|v| v.key == key)
        .unwrap()
        .slots
        .get(&SlotKind::Rmaos)
    {
        Some(sdc_gui::model::sets::SlotFill::Own { item }) => item.clone(),
        other => panic!("RMAOS is {other:?}"),
    };
    let mut h = harness(m);
    h.run();
    assert_eq!(h.query_all_by_label(CAPTION).count(), 0);
    let hover_thumb = |h: &mut Harness<'static, AppModel>| {
        let thumb = h.get_by_label("RMAOS thumbnail").rect().center();
        h.input_mut()
            .events
            .push(eframe::egui::Event::PointerMoved(thumb));
        h.run_steps(90);
    };
    // Unlocked, the thumbnail's hover is its path.
    hover_thumb(&mut h);
    assert_eq!(h.query_all_by_label(&rmaos).count(), 1, "the path hover");
    h.input_mut()
        .events
        .push(eframe::egui::Event::PointerMoved(eframe::egui::pos2(
            5.0, 890.0,
        )));
    h.run_steps(10);
    // The caption sits over the thumbnail: the card's content below it does not move.
    let empties = |h: &Harness<'static, AppModel>| -> Vec<_> {
        h.query_all_by_label("empty").map(|n| n.rect()).collect()
    };
    let before = empties(&h);
    assert!(!before.is_empty());
    outside_sets(&h, "RMAOS").click_secondary();
    h.run();
    h.get_by_label(KEEP).click();
    h.run();
    assert_eq!(settings_of(h.state()).get("lock_rmaos"), Some(&json!(true)));
    // No caption over the thumbnail: the cross alone.
    assert_eq!(h.query_all_by_label_contains(CAPTION).count(), 0);
    assert_eq!(empties(&h), before);
    // One hover on the locked card: the lock, not also the thumbnail's path.
    hover_thumb(&mut h);
    assert_eq!(h.query_all_by_label_contains(HOVER).count(), 1);
    assert_eq!(
        h.query_all_by_label(&rmaos).count(),
        0,
        "the path hover is hidden"
    );
}

/// The scan database `used_by_meshes.rs` builds, with one texture set: `diffuse` in slot 0 and `normal` in slot 1.
fn db_with_set(db: &std::path::Path, diffuse: &str, normal: &str) {
    let s = modscan::store::Store::open(db).unwrap();
    let c = s.conn();
    c.execute(
        "INSERT INTO path(id, p) VALUES (1, ?1), (2, ?2)",
        [diffuse, normal],
    )
    .unwrap();
    c.execute_batch(
        "INSERT INTO texset(id, n, t0, t1) VALUES (1, 2, 1, 2);
         INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0), (2, 1, 1);
         INSERT INTO path(id, p) VALUES (3, 'meshes/a.nif');
         INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (1, 1, 'A', 'a', 1, 1);
         INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (1, 3, 1, 0, 0);
         INSERT INTO shape(copy_id, block, kind, shader_kind, texset_id) VALUES (1, 0, 'BSTriShape', 1, 1);",
    )
    .unwrap();
}

#[test]
fn the_original_textures_are_read_off_the_ui_thread_and_again_after_a_scan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert_eq!(m.original_textures(SET, 2), None, "no scan");
    // The diffuse's scan path, as the Used-by list asks for it.
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    db_with_set(&db, "x.dds", "y.dds");
    m.scan_db = Some(db.clone());
    let t = std::time::Instant::now();
    let tex = loop {
        m.poll();
        if let Some(Some(u)) = m.used_by_for("landscape/rock01.png") {
            break u.tex.clone();
        }
        assert!(t.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
    };
    let db = dbdir.path().join("scan2.db");
    db_with_set(&db, &tex, "textures/landscape/rock01_n.dds");
    m.scan_db = Some(db);
    assert_eq!(m.original_textures(SET, 2), Some(None), "not read yet");
    m.ask_original_textures(SET, 2);
    assert!(m.used_by_pending(), "frames keep coming while it reads");
    let t = std::time::Instant::now();
    let list = loop {
        m.poll();
        if let Some(Some(l)) = m.original_textures(SET, 2) {
            break l;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "the read never landed"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(*list, vec!["textures/landscape/rock01_n.dds".to_string()]);
    // A finished scan reads again.
    m.refresh_mesh_votes(false);
    assert_eq!(m.original_textures(SET, 2), Some(None));
}

/// Coordinator ruling: `path_contains` on a renamed set is not written (PGPatcher would mangle its hits' paths) and
/// cannot be added; one already there stays editable.
#[test]
fn a_renamed_set_writes_no_path_contains_and_refuses_a_new_one() {
    let d = r"landscape\rock01";
    let pairs = [
        ("path_contains", json!("rock")),
        ("rename", json!("boulder")),
    ];
    assert_eq!(
        written(&pairs, &[]),
        json!({ "match_diffuse": r"landscape\rock01", "rename": r"landscape\boulder" })
    );
    let w = pitfalls(&s(&pairs), d);
    assert!(
        w.iter().any(|m| m.contains("path_contains is not written")),
        "{w:?}"
    );
    let renamed = s(&[("rename", json!("boulder"))]);
    assert_eq!(
        extra_match_refusal(&renamed, d, "path_contains", Some(&json!("rock"))),
        Some(RENAMED_ONE_MATCH)
    );
    assert_eq!(
        extra_match_refusal(&s(&pairs), d, "path_contains", Some(&json!("rocks"))),
        None,
        "editing the one already there"
    );
    assert_eq!(
        extra_match_refusal(&s(&[]), d, "path_contains", Some(&json!("rock"))),
        None
    );
    // Not renamed: written as before.
    assert_eq!(written(&pairs[..1], &[])["path_contains"], json!("rock"));
}

#[test]
fn the_lock_entry_is_off_on_a_set_that_writes_no_pgpatcher_entry() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let menu = m.slot_menu("landscape/leaf", SlotKind::Diffuse);
    let e = menu
        .iter()
        .find(|e| matches!(e.action, MenuAction::PgpLock { .. }))
        .expect("listed");
    assert!(
        e.disabled
            .as_deref()
            .is_some_and(|w| w.contains("no PGPatcher entry")),
        "{e:?}"
    );
}

/// Review M3: an explicit slot command next to its lock is still written (PGPatcher applies it after the lock).
#[test]
fn an_explicit_slot_command_next_to_its_lock_is_written() {
    let v = written(
        &[
            ("lock_normal", json!(true)),
            ("slot2", json!(r"textures\a_n.dds")),
        ],
        &[(2, r"textures\a_n.dds")],
    );
    assert_eq!(v["lock_normal"], json!(true));
    assert_eq!(v["slot2"], json!(r"textures\a_n.dds"));
}

/// Review M1: a read that panicked keeps its error as the answer, so it is not started again every frame.
#[test]
fn a_panicked_original_textures_read_keeps_its_error() {
    use sdc_gui::worker::WorkerMsg;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = ("textures/x.dds".to_string(), 2u8);
    m.original.pending.insert(key.clone());
    m.sender()
        .send(WorkerMsg::Panicked {
            what: "original-textures",
            message: "boom".into(),
        })
        .unwrap();
    m.poll();
    assert!(m.original.pending.is_empty(), "no longer reading");
    let e = m.original.done.get(&key).expect("the error is the answer");
    assert!(e[0].contains("boom"), "{e:?}");
}

/// Review M2: a lock key on a set that writes no JSON gets a note, not the cross.
#[test]
fn a_lock_on_a_set_without_json_shows_a_note_not_the_cross() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let leaf = "landscape/leaf";
    m.set_json_value(leaf, "lock_diffuse", Some(json!(true)));
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(leaf))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    assert_eq!(
        settings_of_set(h.state(), leaf).get("lock_diffuse"),
        Some(&json!(true))
    );
    assert_eq!(h.query_all_by_label(CAPTION).count(), 0);
    assert_eq!(
        h.query_all_by_label("lock stored, not written (no PGPatcher entry)")
            .count(),
        1
    );
}

fn settings_of_set(m: &AppModel, id: &str) -> Settings {
    set_views(m)
        .into_iter()
        .find(|v| v.id == id)
        .unwrap()
        .settings
}

/// Trace-debug #42: four texture sets with diffuse `d.dds`; only the first is used by a lighting shape in a
/// non-derived copy of an enabled mod. The others: a disabled mod, an effect shape, a PGPatcher output copy.
fn db_with_filtered_users(db: &std::path::Path) {
    let s = modscan::store::Store::open(db).unwrap();
    s.conn()
        .execute_batch(
            "INSERT INTO path(id, p) VALUES (1, 'textures/d.dds'), (2, 'textures/ok_n.dds'),
               (3, 'textures/off_n.dds'), (4, 'textures/grey.dds'), (5, 'textures/pgp_n.dds'),
               (11, 'meshes/a.nif'), (12, 'meshes/b.nif'), (13, 'meshes/c.nif'), (14, 'meshes/d.nif');
             INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES
               (1, 1, 'On', 'on', 1, 1), (2, 1, 'Off', 'off', 2, 0);
             INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES
               (1, 11, 1, 0, 0), (2, 12, 2, 0, 0), (3, 13, 1, 0, 0), (4, 14, 1, 0, 1);
             INSERT INTO texset(id, n, t0, t1) VALUES (1, 2, 1, 2), (2, 2, 1, 3), (3, 2, 1, 4), (4, 2, 1, 5);
             INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES
               (1, 1, 0), (2, 1, 1), (1, 2, 0), (3, 2, 1), (1, 3, 0), (4, 3, 1), (1, 4, 0), (5, 4, 1);
             INSERT INTO shape(copy_id, block, kind, shader_kind, texset_id) VALUES
               (1, 0, 'BSTriShape', 1, 1), (2, 0, 'BSTriShape', 1, 2), (3, 0, 'BSTriShape', 2, 3),
               (4, 0, 'BSTriShape', 1, 4);",
        )
        .unwrap();
}

#[test]
fn slot_textures_skip_disabled_effect_and_derived_shapes() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    db_with_filtered_users(&db);
    assert_eq!(
        sdc_gui::model::used_by::slot_textures(&db, "textures/d.dds", 2).unwrap(),
        ["textures/ok_n.dds"]
    );
    let known = sdc_gui::model::known_paths::read(&db).unwrap();
    assert_eq!(
        known
            .textures
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["textures/d.dds", "textures/ok_n.dds"]
    );
}
