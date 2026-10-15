//! DECISIONS AJ7-AJ11: a set's feature keys turn on by themselves from its member textures (the "(auto)" layer);
//! an explicit value always wins and nothing is stored.
mod support;
use sdc_gui::model::auto_flags::{auto_flags, resolved};
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::sets::{SlotFill, SlotKind};
use sdc_gui::pgp::settings::Settings;
use sdc_gui::pgp::writer::{document, to_text};
use serde_json::json;
use std::collections::BTreeMap;
use support::*;

fn filled(kinds: &[SlotKind]) -> BTreeMap<SlotKind, SlotFill> {
    kinds
        .iter()
        .map(|k| (*k, SlotFill::Own { item: "x".into() }))
        .collect()
}

fn keys(kinds: &[SlotKind], own: &Settings) -> Vec<&'static str> {
    auto_flags(true, &filled(kinds), own)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

#[test]
fn each_member_turns_its_features_on() {
    let none = Settings::new();
    assert_eq!(
        keys(&[SlotKind::Coat], &none),
        ["multilayer", "coat_normal"]
    );
    assert_eq!(keys(&[SlotKind::Fuzz], &none), ["fuzz_on"]);
    assert_eq!(keys(&[SlotKind::Height], &none), ["parallax"]);
    assert_eq!(keys(&[SlotKind::Emissive], &none), ["emissive"]);
    assert_eq!(keys(&[SlotKind::Subsurface], &none), ["subsurface"]);
    // `_s` is the coat diffuse when multilayer is on, from `_cnr` or by hand.
    assert_eq!(
        keys(&[SlotKind::Coat, SlotKind::Subsurface], &none),
        ["multilayer", "coat_normal", "coat_diffuse"]
    );
    let mut ml = Settings::new();
    ml.insert("multilayer".into(), json!(true));
    assert_eq!(keys(&[SlotKind::Subsurface], &ml), ["coat_diffuse"]);
    assert!(keys(&[], &none).is_empty());
    assert!(auto_flags(false, &filled(&[SlotKind::Coat]), &none).is_empty());
}

#[test]
fn the_why_names_the_member() {
    let a = auto_flags(true, &filled(&[SlotKind::Coat]), &Settings::new());
    assert_eq!(a[0].1, "on because the set has a _cnr");
}

#[test]
fn an_explicit_value_beats_auto_including_off() {
    let mut own = Settings::new();
    own.insert("multilayer".into(), json!(false));
    let k = keys(&[SlotKind::Coat], &own);
    assert_eq!(k, ["coat_normal"]);
    let r = resolved(true, &filled(&[SlotKind::Coat]), &own);
    assert_eq!(r.get("multilayer"), Some(&json!(false)));
    assert_eq!(r.get("coat_normal"), Some(&json!(true)));
}

#[test]
fn a_feature_an_explicit_one_rules_out_stays_off() {
    let mut own = Settings::new();
    own.insert("hair".into(), json!(true));
    assert!(keys(&[SlotKind::Coat, SlotKind::Fuzz], &own)
        .iter()
        .all(|k| *k == "coat_normal"));
}

fn text(m: &sdc_gui::model::AppModel) -> String {
    let e = json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .1;
    to_text(&document(&[e]))
}

#[test]
fn the_writer_uses_the_resolved_value() {
    let d = tempfile::tempdir().unwrap();
    mod_folder(d.path());
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([128u8, 128, 255]))
        .save(d.path().join("textures/landscape/rock01_cnr.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    let t = text(&m);
    assert!(t.contains("\"multilayer\": true"), "{t}");
    assert!(t.contains("\"coat_normal\": true"), "{t}");
    m.set_json_value("landscape/rock01", "multilayer", Some(json!(false)));
    assert!(!text(&m).contains("\"multilayer\""));
}

#[test]
fn without_the_member_nothing_is_written() {
    let d = tempfile::tempdir().unwrap();
    let t = text(&pbr_opened(d.path()));
    assert!(
        !t.contains("\"multilayer\"") && !t.contains("\"coat_normal\""),
        "{t}"
    );
}
