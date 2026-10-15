//! Task 10b: the Advanced skin card (owner, 2026-10-14). Per-set, non-inheritable CS SkinProfile values kept in
//! the set's `skin` table of `sdc-project.toml`; nothing is exported yet (AG79-AG86).
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::Subtype;
use sdc_gui::model::skin::FIELDS;
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::time::Duration;
use support::*;

const SKIN: &str = "landscape/rock01";
const TITLE: &str = "Advanced skin (Community Shaders)";

fn skin_opened(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::SkinAlbedo));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn set_key(m: &AppModel) -> String {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id == SKIN)
        .unwrap()
        .key
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1400.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_field_list_is_the_twenty_cs_keys_without_wetness() {
    let keys: Vec<&str> = FIELDS.iter().map(|f| f.key).collect();
    assert_eq!(
        keys,
        [
            "SkinMainRoughness",
            "SkinSecondRoughness",
            "SkinSpecularTexMultiplier",
            "SecondarySpecularStrength",
            "F0",
            "BaseColorMultiplier",
            "PhysicalMainRoughnessMultiplier",
            "PhysicalSecondRoughnessMultiplier",
            "PhysicalSpecularStrength",
            "ExtraEdgeRoughness",
            "EnableSkinDetail",
            "SkinDetailStrength",
            "SkinDetailTiling",
            "BodyTilingMultiplier",
            "Translucency",
            "sssWidth",
            "UseSSS",
            "FuzzStrength",
            "FuzzRoughness",
            "FuzzF0",
        ]
    );
    assert!(!keys.contains(&"WetResponse"));
}

#[test]
fn the_card_shows_for_a_skin_set_and_not_for_a_plain_one() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains(TITLE);
    h.get_by_label_contains("export spec pending");

    let d2 = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d2.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_label_contains(TITLE).is_none());
}

#[test]
fn an_unset_field_shows_its_default_and_stores_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    assert!(m.skin_values(SKIN).is_empty());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label("default 0.7");
    h.get_by_label("default 0.0278");
    assert!(h.state().skin_values(SKIN).is_empty());
}

#[test]
fn setting_resetting_and_one_undo_step_each() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    m.set_skin_value(SKIN, "F0", Some(json!(0.04)));
    m.set_skin_value(SKIN, "UseSSS", Some(json!(false)));
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.undo();
    assert!(!m.skin_values(SKIN).contains_key("UseSSS"));
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.set_skin_value(SKIN, "F0", None);
    assert!(m.skin_values(SKIN).is_empty(), "reset clears it");
    m.undo();
    assert_eq!(m.skin_values(SKIN)["F0"], json!(0.04));
    m.undo();
    assert!(
        m.skin_values(SKIN).is_empty(),
        "the set itself was one step"
    );
}

#[test]
fn the_skin_table_round_trips_and_unknown_keys_survive() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    m.set_skin_value(SKIN, "F0", Some(json!(0.04)));
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value(SKIN, "skin.Future", Some(json!("x")));
    m.set_skin_value(SKIN, "sssWidth", Some(json!(0.3)));
    let root = m.root.clone().unwrap();
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let s = SetsDoc::open(&root);
    let e = s.entries().into_iter().find(|e| e.id == SKIN).unwrap();
    assert_eq!(e.skin["F0"], json!(0.04));
    assert_eq!(e.skin["sssWidth"], json!(0.3));
    assert_eq!(e.skin["Future"], json!("x"));
    assert!(
        e.settings.keys().all(|k| !k.starts_with("skin")),
        "skin values are not PGPatcher settings: {:?}",
        e.settings
    );
}

#[test]
fn the_set_button_stores_the_default_and_the_reset_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.query_all_by_label("set").next().unwrap().click();
    h.run();
    assert_eq!(h.state().skin_values(SKIN)["SkinMainRoughness"], json!(0.7));
    h.get_by_label("↺").click();
    h.run();
    assert!(h.state().skin_values(SKIN).is_empty());
}

// ---- Task 10 fix round 1: the 10b review carry-over and the owner's no-JSON ruling for skin sets (AG88-AG90)

#[test]
fn every_default_group_and_range_is_the_briefs_table() {
    use sdc_gui::model::skin::Kind;
    let want: [(&str, &str, Kind); 20] = [
        ("SkinMainRoughness", "Specular", n(0.7, 0.0, 1.0)),
        ("SkinSecondRoughness", "Specular", n(0.35, 0.0, 1.0)),
        ("SkinSpecularTexMultiplier", "Specular", n(1.0, 0.0, 4.0)),
        ("SecondarySpecularStrength", "Specular", n(0.15, 0.0, 1.0)),
        ("F0", "Specular", n(0.0278, 0.0, 1.0)),
        ("BaseColorMultiplier", "Physical", n(1.0, 0.0, 4.0)),
        (
            "PhysicalMainRoughnessMultiplier",
            "Physical",
            n(1.3, 0.0, 4.0),
        ),
        (
            "PhysicalSecondRoughnessMultiplier",
            "Physical",
            n(0.75, 0.0, 4.0),
        ),
        ("PhysicalSpecularStrength", "Physical", n(1.0, 0.0, 4.0)),
        ("ExtraEdgeRoughness", "Edge & detail", n(0.25, 0.0, 1.0)),
        (
            "EnableSkinDetail",
            "Edge & detail",
            Kind::Flag { default: true },
        ),
        ("SkinDetailStrength", "Edge & detail", n(0.25, 0.0, 1.0)),
        ("SkinDetailTiling", "Edge & detail", n(10.0, 0.1, 100.0)),
        ("BodyTilingMultiplier", "Edge & detail", n(2.0, 0.0, 4.0)),
        ("Translucency", "SSS", n(0.1, 0.0, 1.0)),
        ("sssWidth", "SSS", n(0.2, 0.0, 1.0)),
        ("UseSSS", "SSS", Kind::Flag { default: true }),
        ("FuzzStrength", "Fuzz", n(1.0, 0.0, 1.0)),
        ("FuzzRoughness", "Fuzz", n(0.35, 0.0, 1.0)),
        ("FuzzF0", "Fuzz", n(0.045, 0.0, 1.0)),
    ];
    for (f, (key, group, kind)) in FIELDS.iter().zip(want) {
        assert_eq!((f.key, f.group, f.kind), (key, group, kind), "{key}");
    }
}

fn n(default: f64, lo: f64, hi: f64) -> sdc_gui::model::skin::Kind {
    sdc_gui::model::skin::Kind::Num { default, lo, hi }
}

#[test]
fn a_drag_on_one_field_is_one_undo_step_and_is_rounded() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    for x in [0.70, 0.705, 0.7100000000000001] {
        m.set_skin_value(SKIN, "SkinMainRoughness", Some(json!(x)));
    }
    assert_eq!(m.skin_values(SKIN)["SkinMainRoughness"], json!(0.71));
    m.undo();
    assert!(m.skin_values(SKIN).is_empty(), "the drag was one step");
}

#[test]
fn unknown_skin_keys_and_comments_survive_a_real_load_and_save() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    m.set_skin_value(SKIN, "F0", Some(json!(0.04)));
    let root = m.root.clone().unwrap();
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let path = root.join(sdc_gui::sets_doc::FILE_NAME);
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.replacen(
        "skin.F0 = 0.04",
        "# kept by hand\nskin.Future = \"x\"\nskin.F0 = 0.04",
        1,
    );
    assert!(text.contains("Future"), "{text}");
    std::fs::write(&path, &text).unwrap();
    // The real load path: open the folder again.
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(
        m.skin_values(SKIN).get("Future"),
        Some(&json!("x")),
        "{text}
{:?}",
        m.skin_values(SKIN)
    );
    m.set_skin_value(SKIN, "F0", Some(json!(0.05)));
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let out = std::fs::read_to_string(&path).unwrap();
    assert!(out.contains("# kept by hand"), "{out}");
    assert!(out.contains("Future = \"x\""), "{out}");
    assert!(out.contains("F0 = 0.05"), "{out}");
    // Not a PGPatcher setting: no JSON entry carries it.
    for (_, e) in sdc_gui::model::pbr_export::json_entries(&m) {
        assert!(
            e.settings.keys().all(|k| !k.contains("Future")),
            "{:?}",
            e.settings
        );
    }
}

#[test]
fn a_skin_set_has_no_json_entry_no_truepbr_card_and_an_export_placeholder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = skin_opened(d.path());
    assert!(sdc_gui::model::pbr_export::json_entries(&m).is_empty());
    assert!(sdc_gui::model::pbr_export::export_blocks(&m)
        .iter()
        .all(|b| !b.set_id.contains("rock01")));
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains(TITLE);
    assert!(
        h.query_by_label("Specular level").is_none(),
        "no TruePBR card"
    );
    h.get_by_label("Advanced skin (no JSON)");
    {
        use egui_kittest::kittest::NodeT;
        assert!(h
            .get_by_label("Export JSON…")
            .accesskit_node()
            .is_disabled());
    }

    // A PBR set is unchanged.
    let d2 = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d2.path());
    assert_eq!(sdc_gui::model::pbr_export::json_entries(&m).len(), 1);
    let k = set_key(&m);
    m.select_set(&k);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Specular level");
    h.get_by_label("PBR → JSON");
}

#[test]
fn an_old_skin_entry_drops_at_the_next_export_under_the_keep_rules() {
    use sdc_gui::model::export::{Row, RowState};
    use sdc_gui::model::pbr_export::{finish, prepare, Finish};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::SkinAlbedo));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let views = sdc_gui::model::sets::set_views(&m);
    assert!(views.iter().any(|v| v.skin), "leaf is a skin set");
    let p = prepare(&m).expect("the PBR set is written");
    assert_eq!(p.entries.len(), 1, "only the PBR set");
    std::fs::create_dir_all(p.path.parent().unwrap()).unwrap();
    std::fs::write(&p.path, "{\"entries\": [\"landscape\\\\leaf skin\"]}").unwrap();
    let rows: Vec<Row> = p
        .entries
        .iter()
        .flat_map(|(_, deps, _)| deps.clone())
        .map(|job_key| Row {
            job_key,
            path: String::new(),
            state: RowState::Done { passed: true },
            warnings: vec![],
        })
        .collect();
    // Edited outside the GUI: the overwrite question, with text that has no skin entry.
    let mut notes = vec![];
    match finish(&p, &rows, None, &mut notes) {
        Finish::Overwrite(t) => assert!(!t.contains("leaf"), "{t}"),
        r => panic!("{r:?} {notes:?}"),
    }
    // Written by us last time: replaced, and the skin entry is gone.
    let last = sdc_gui::model::history::sha256_of(&p.path);
    assert!(matches!(
        finish(&p, &rows, last.as_deref(), &mut notes),
        Finish::Written(_)
    ));
    assert!(!std::fs::read_to_string(&p.path).unwrap().contains("leaf"));
}
