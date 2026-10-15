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
fn a_locked_slot_card_shows_the_caption() {
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
    let mut h = harness(m);
    h.run();
    assert_eq!(h.query_all_by_label(CAPTION).count(), 0);
    h.get_by_label("RMAOS").click_secondary();
    h.run();
    h.get_by_label(KEEP).click();
    h.run();
    assert_eq!(settings_of(h.state()).get("lock_rmaos"), Some(&json!(true)));
    assert_eq!(h.query_all_by_label(CAPTION).count(), 1);
}
