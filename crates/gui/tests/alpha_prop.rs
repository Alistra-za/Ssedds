//! Task 4b: the NiAlphaProperty keys of the PGPatcher schema (AG27-AG31): flag bits, writer, pitfalls, the card.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::pgp::alpha::{AlphaFlags, BLEND_MODES, TEST_FUNCS};
use sdc_gui::pgp::prefix::pitfalls;
use sdc_gui::pgp::schema::{spec, KeyKind, NumRange};
use sdc_gui::pgp::settings::Settings;
use sdc_gui::pgp::validate::validate;
use sdc_gui::pgp::writer::{document, EntryInput};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use support::*;

const SET: &str = "landscape/rock01";
const DERIVED: &str = r"landscape\rock01";

#[test]
fn flags_decode_as_nifskope_does() {
    let f = AlphaFlags::from_raw(4844);
    assert!(!f.blend && f.test && !f.no_sorter);
    assert_eq!((f.src, f.dst, f.func), (6, 7, 4));
    assert_eq!(BLEND_MODES[6], "Src Alpha");
    assert_eq!(BLEND_MODES[7], "Inv Src Alpha");
    assert_eq!(TEST_FUNCS[4], "Greater");
    assert_eq!((BLEND_MODES.len(), TEST_FUNCS.len()), (11, 8));
    assert_eq!(f.raw(), 4844);
}

#[test]
fn every_field_round_trips_and_the_top_bits_are_kept() {
    for raw in [0u16, 1, 0xFFFF, 0xC000 | 4844, 1 << 13, 0x3FFF, 4844] {
        assert_eq!(AlphaFlags::from_raw(raw).raw(), raw, "{raw}");
    }
    let mut f = AlphaFlags::from_raw(0xC000);
    f.blend = true;
    assert_eq!(f.raw(), 0xC001);
    f = AlphaFlags::from_raw(0xC000);
    f.src = 10;
    f.dst = 10;
    f.func = 7;
    f.test = true;
    f.no_sorter = true;
    assert_eq!(
        f.raw(),
        0xC000 | (10 << 1) | (10 << 5) | (1 << 9) | (7 << 10) | (1 << 13)
    );
    // A field cannot spill into its neighbour.
    f.src = 0xFF;
    assert_eq!(AlphaFlags::from_raw(f.raw()).dst, 10);
}

#[test]
fn the_keys_are_in_the_table() {
    assert_eq!(
        spec("alphaprop_flags").unwrap().kind,
        KeyKind::Number(NumRange::Int(65535))
    );
    assert_eq!(
        spec("alphaprop_thresh").unwrap().kind,
        KeyKind::Number(NumRange::Int(255))
    );
    assert_eq!(spec("alphaprop_create").unwrap().kind, KeyKind::OnlyTrue);
    assert_eq!(spec("alphaprop_delete").unwrap().kind, KeyKind::OnlyTrue);
}

fn s(pairs: &[(&str, Value)]) -> Settings {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn written(pairs: &[(&str, Value)]) -> (String, Value) {
    let e = EntryInput {
        settings: s(pairs),
        slots: BTreeMap::new(),
        derived_match: DERIVED.into(),
        section: false,
    };
    let doc = document(std::slice::from_ref(&e));
    let v = doc.to_value();
    validate(&v).expect("the schema accepts the entry");
    (sdc_gui::pgp::writer::to_text(&doc), v["entries"][0].clone())
}

#[test]
fn the_writer_emits_integers_and_booleans_in_schema_order() {
    let (text, v) = written(&[
        ("alphaprop_delete", json!(true)),
        ("alphaprop_create", json!(true)),
        ("alphaprop_thresh", json!(128)),
        ("alphaprop_flags", json!(4844)),
        ("emissive", json!(true)),
    ]);
    assert_eq!(v["alphaprop_flags"], json!(4844));
    assert!(v["alphaprop_flags"].is_u64() && v["alphaprop_thresh"].is_u64());
    assert_eq!(v["alphaprop_create"], json!(true));
    let at = |k: &str| text.find(&format!("\"{k}\"")).unwrap();
    assert!(at("alphaprop_flags") < at("alphaprop_thresh"));
    assert!(at("alphaprop_thresh") < at("alphaprop_create"));
    assert!(at("alphaprop_create") < at("alphaprop_delete"));
    // The fixed default block also names emissive, earlier in the file; the entry's own comes last.
    assert!(at("alphaprop_delete") < text.rfind("\"emissive\"").unwrap());
}

#[test]
fn a_key_that_is_off_is_not_written_and_a_wild_value_is_clamped() {
    let (_, v) = written(&[
        ("alphaprop_create", json!(false)),
        ("alphaprop_delete", json!(false)),
    ]);
    assert!(v.get("alphaprop_create").is_none() && v.get("alphaprop_delete").is_none());
    let (_, v) = written(&[
        ("alphaprop_flags", json!(70000)),
        ("alphaprop_thresh", json!(-3)),
    ]);
    assert_eq!(
        (v["alphaprop_flags"].clone(), v["alphaprop_thresh"].clone()),
        (json!(65535), json!(0))
    );
}

#[test]
fn both_pitfalls_fire() {
    let none = pitfalls(&s(&[]), DERIVED);
    assert!(none
        .iter()
        .all(|w| !w.contains("NiAlphaProperty") && !w.contains("Delete wins")));
    for k in [
        ("alphaprop_flags", json!(4844)),
        ("alphaprop_thresh", json!(10)),
    ] {
        let w = pitfalls(&s(&[k]), DERIVED);
        assert!(
            w.iter()
                .any(|w| w.contains("only changes shapes that already have an NiAlphaProperty")),
            "{w:?}"
        );
    }
    let w = pitfalls(
        &s(&[
            ("alphaprop_flags", json!(4844)),
            ("alphaprop_create", json!(true)),
        ]),
        DERIVED,
    );
    assert!(w.iter().all(|w| !w.contains("already have")), "{w:?}");
    let w = pitfalls(
        &s(&[
            ("alphaprop_delete", json!(true)),
            ("alphaprop_thresh", json!(9)),
        ]),
        DERIVED,
    );
    assert!(
        w.iter()
            .any(|w| w.contains("Delete wins; the other alpha keys of this entry do nothing")),
        "{w:?}"
    );
    let w = pitfalls(&s(&[("alphaprop_delete", json!(true))]), DERIVED);
    assert!(w.iter().all(|w| !w.contains("Delete wins")), "{w:?}");
}

fn panel(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(800.0, 1400.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn stored(m: &AppModel, key: &str) -> Option<Value> {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .into_iter()
        .find(|e| e.id == SET)?
        .settings
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

fn rock(dir: &std::path::Path) -> AppModel {
    let mut m = pbr_opened(dir);
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    m
}

#[test]
fn testing_flips_bit_9_in_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "alphaprop_flags", Some(json!(4844)));
    m.set_json_value(SET, "alphaprop_create", Some(json!(true)));
    let mut h = panel(m);
    h.run();
    h.get_by_label("Enable testing").click();
    h.run();
    assert_eq!(
        stored(h.state(), "alphaprop_flags"),
        Some(json!(4844 ^ 512))
    );
    h.state_mut().undo();
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(4844)));
}

#[test]
fn the_override_and_threshold_boxes_write_their_defaults() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    // Opens the section.
    m.set_json_value(SET, "alphaprop_create", Some(json!(true)));
    let mut h = panel(m);
    h.run();
    h.get_by_label_contains("Override flags").click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(4844)));
    h.get_by_label_contains("Alpha test threshold").click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_thresh"), Some(json!(128)));
    h.get_by_label_contains("Override flags").click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_flags"), None);
}

#[test]
fn delete_greys_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "alphaprop_flags", Some(json!(4844)));
    m.set_json_value(SET, "alphaprop_delete", Some(json!(true)));
    let mut h = panel(m);
    h.run();
    assert!(h
        .get_by_label("Enable testing")
        .accesskit_node()
        .is_disabled());
    assert!(!h
        .get_by_label_contains("Delete NiAlphaProperty")
        .accesskit_node()
        .is_disabled());
}
