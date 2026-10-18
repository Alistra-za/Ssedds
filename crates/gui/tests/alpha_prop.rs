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
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(800.0, 1400.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    // Layout v2 A3: the group starts collapsed, set or not.
    h.run();
    h.get_by_label("NiAlphaProperty").click();
    h
}

fn stored(m: &AppModel, key: &str) -> Option<Value> {
    m.sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .find(|&e| e.id == SET)
        .cloned()?
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

// ---- Task 5 carry-over (Task 4b review) ----

const TEXT: eframe::egui::accesskit::Role = eframe::egui::accesskit::Role::TextInput;

fn raw_box_shows(h: &Harness<'static, AppModel>, shown: &str) -> bool {
    h.query_all_by_role(TEXT)
        .any(|n| n.value().as_deref() == Some(shown))
}

/// Focus the raw box (it shows `shown`), select all, type `text`, then press `end`.
fn type_raw(h: &mut Harness<'static, AppModel>, shown: &str, text: &str, end: eframe::egui::Key) {
    h.query_all_by_role(TEXT)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap_or_else(|| panic!("no box shows {shown}"))
        .focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.run();
    h.query_all_by_role(TEXT)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap()
        .type_text(text);
    h.run();
    h.key_press(end);
    h.run();
    h.run();
}

fn with_flags(dir: &std::path::Path, flags: Value) -> Harness<'static, AppModel> {
    let mut m = rock(dir);
    m.set_json_value(SET, "alphaprop_flags", Some(flags));
    let mut h = panel(m);
    h.run();
    h
}

#[test]
fn the_raw_box_commits_a_valid_number_refuses_an_invalid_one_and_esc_cancels() {
    let d = tempfile::tempdir().unwrap();
    let mut h = with_flags(d.path(), json!(4844));
    type_raw(&mut h, "4844", "5000", eframe::egui::Key::Enter);
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(5000)));
    // Too big for 16 bits: nothing is written and the box goes back to the value.
    type_raw(&mut h, "5000", "70000", eframe::egui::Key::Enter);
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(5000)));
    assert!(raw_box_shows(&h, "5000"), "the box reverts");
    type_raw(&mut h, "5000", "123", eframe::egui::Key::Escape);
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(5000)));
    assert!(raw_box_shows(&h, "5000"));
}

#[test]
fn a_stale_raw_buffer_is_not_shown_after_the_section_was_closed() {
    let d = tempfile::tempdir().unwrap();
    let mut h = with_flags(d.path(), json!(4844));
    h.query_all_by_role(TEXT)
        .find(|n| n.value().as_deref() == Some("4844"))
        .unwrap()
        .focus();
    h.run();
    // A text event: the box now holds "48449", which is not the stored value.
    h.query_all_by_role(TEXT)
        .find(|n| n.value().as_deref() == Some("4844"))
        .unwrap()
        .type_text("9");
    h.run();
    // The section collapses while the box is focused, then opens again many frames later.
    h.get_by_label("NiAlphaProperty").click();
    for _ in 0..5 {
        h.run();
    }
    h.get_by_label("NiAlphaProperty").click();
    h.run();
    h.run();
    assert!(
        raw_box_shows(&h, "4844"),
        "the buffer from long ago is gone"
    );
}

#[test]
fn the_blend_and_function_dropdowns_edit_their_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut h = with_flags(d.path(), json!(4844));
    let role = eframe::egui::accesskit::Role::ComboBox;
    // Source blend, destination blend and test function (the Subsurface selector sits in its closed group, A3).
    for (n, pick) in [(0, "One"), (1, "Zero"), (2, "Less")] {
        h.query_all_by_role(role).nth(n).unwrap().click();
        h.run();
        h.get_by_label(pick).click();
        h.run();
        h.run();
    }
    let raw = stored(h.state(), "alphaprop_flags")
        .unwrap()
        .as_u64()
        .unwrap() as u16;
    let f = AlphaFlags::from_raw(raw);
    assert_eq!((f.src, f.dst, f.func), (0, 1, 1));
    assert!(f.test && !f.blend, "nothing else moved");
    assert_eq!(raw, (1 << 5) | (1 << 9) | (1 << 10));
}

#[test]
fn each_alpha_key_has_a_reset_and_a_whole_float_reads_as_an_integer() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "alphaprop_flags", Some(json!(4844.0)));
    m.set_json_value(SET, "alphaprop_thresh", Some(json!(200)));
    m.set_json_value(SET, "alphaprop_create", Some(json!(true)));
    let mut h = panel(m);
    h.run();
    assert!(raw_box_shows(&h, "4844"), "4844.0 shows as 4844");
    assert!(h.query_by_label_contains("is not valid").is_none());
    assert_eq!(h.query_all_by_label("reset").count(), 3, "one per key set");
    h.query_all_by_label("reset").next().unwrap().click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_flags"), None);
    h.state_mut().undo();
    assert!(
        stored(h.state(), "alphaprop_flags").is_some(),
        "one undo step"
    );
}

#[test]
fn a_negative_or_fractional_value_is_invalid_not_unset() {
    let d = tempfile::tempdir().unwrap();
    let h = with_flags(d.path(), json!(-5));
    h.get_by_label_contains("stored -5 is not valid; clamped to a whole number 0-65535");
    assert_eq!(
        h.get_by_label("Override flags").accesskit_node().toggled(),
        Some(eframe::egui::accesskit::Toggled::True),
        "the key is set, so its box is ticked"
    );
    drop(h);
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "alphaprop_thresh", Some(json!(1.5)));
    let mut h = panel(m);
    h.run();
    h.get_by_label_contains("stored 1.5 is not valid; clamped to a whole number 0-255");
}

#[test]
fn the_json_editor_reads_integers_as_floats_too() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(SET, "alphaprop_thresh", Some(json!(-2)));
    m.set_json_value(SET, "alphaprop_flags", Some(json!(4844.0)));
    m.json_editor_open = true;
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::json_editor::draw(ui.ctx(), m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label_contains("stored -2 is not valid; clamped to a whole number 0-255");
    assert!(
        h.query_by_label_contains("stored 4844").is_none(),
        "4844.0 is valid"
    );
}

#[test]
fn the_validator_refuses_a_value_outside_minimum_and_maximum() {
    let doc = |flags: Value| {
        let mut d = document(&[EntryInput {
            settings: s(&[("alphaprop_flags", json!(4844))]),
            slots: BTreeMap::new(),
            derived_match: DERIVED.into(),
            section: false,
        }])
        .to_value();
        d["entries"][0]["alphaprop_flags"] = flags;
        d
    };
    assert!(validate(&doc(json!(4844))).is_ok());
    assert!(validate(&doc(json!(70000))).is_err(), "above the maximum");
    assert!(validate(&doc(json!(-3))).is_err(), "below the minimum");
}

#[test]
fn the_int_note_names_the_range_and_whole_reads_floats() {
    assert_eq!(
        NumRange::Int(65535).note(),
        "clamped to a whole number 0-65535"
    );
    assert_eq!(NumRange::whole(&json!(4844.0), 65535), Some(4844));
    assert_eq!(NumRange::whole(&json!(4844), 65535), Some(4844));
    for bad in [json!(-1), json!(1.5), json!(70000), json!("4"), json!(null)] {
        assert_eq!(NumRange::whole(&bad, 65535), None, "{bad}");
    }
}

#[test]
fn the_delete_hover_is_on_the_delete_checkbox() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "alphaprop_flags", Some(json!(4844)));
    m.set_json_value(SET, "alphaprop_delete", Some(json!(true)));
    let mut h = panel(m);
    h.run();
    h.get_by_label_contains("Delete NiAlphaProperty").hover();
    h.run();
    h.run();
    h.get_by_label_contains("Delete takes priority");
}

#[test]
fn no_sorter_is_last_says_it_has_no_effect_and_round_trips() {
    use sdc_gui::ui::pgp_fields::NO_SORTER_LABEL;
    let d = tempfile::tempdir().unwrap();
    // 0x12ED and 0x32ED: the captured hair, bit 13 off and on (AG131).
    let mut h = with_flags(d.path(), json!(0x32ED));
    let ns = h.get_by_label(NO_SORTER_LABEL);
    let raw = h
        .query_all_by_role(TEXT)
        .find(|n| n.value().as_deref() == Some("13037"))
        .expect("the raw box");
    assert!(ns.rect().top() > raw.rect().top(), "drawn last");
    ns.click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(0x12ED)));
    h.get_by_label(NO_SORTER_LABEL).click();
    h.run();
    assert_eq!(stored(h.state(), "alphaprop_flags"), Some(json!(0x32ED)));
}

/// Trace-debug #57: Esc leaves a TruePBR text box (rename, a match) and a JSON editor box (a match, the JSON name)
/// without writing what was typed (AG10/AG11).
#[test]
fn esc_cancels_truepbr_rename_and_match_edits() {
    let esc = eframe::egui::Key::Escape;
    let d = tempfile::tempdir().unwrap();
    let mut m = rock(d.path());
    m.set_json_value(SET, "rename", Some(json!("old_name")));
    let mut h = panel(m);
    h.run();
    // Rename and Matching are under the advanced header.
    h.get_by_label("Advanced TruePBR settings").click();
    h.run();
    type_raw(&mut h, "old_name", "new_name", esc);
    assert_eq!(stored(h.state(), "rename"), Some(json!("old_name")));
    assert!(raw_box_shows(&h, "old_name"), "the box reverts");
    type_raw(&mut h, DERIVED, r"landscape\other", esc);
    assert_eq!(stored(h.state(), "match1"), None);
    let mut m = h.into_state();
    m.add_section("aaa_one");
    m.set_json_name("mine");
    m.json_editor_open = true;
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1000.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::json_editor::draw(ui.ctx(), m),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    type_raw(&mut h, "aaa_one", "bbb_two", esc);
    assert_eq!(
        h.state().sections()[0].get("match1"),
        Some(&json!("aaa_one"))
    );
    type_raw(&mut h, "mine", "yours", esc);
    assert_eq!(
        h.state().sets_doc.as_ref().unwrap().json_name().as_deref(),
        Some("mine")
    );
}
