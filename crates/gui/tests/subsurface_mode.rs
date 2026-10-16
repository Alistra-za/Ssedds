//! DECISIONS AG101-AG104: the Subsurface Off / Standard / Foliage selector, its auto mode and its pitfalls.
mod support;
use sdc_gui::model::auto_flags::{auto_flags_for, resolved_for, SsHint};
use sdc_gui::model::meshes::mesh_tree_anim;
use sdc_gui::model::sets::{SlotFill, SlotKind};
use sdc_gui::pgp::prefix::subsurface_pitfalls;
use sdc_gui::pgp::settings::{sub_edits, sub_mode, subsurface_group_open, Settings, SubMode};
use sdc_gui::pgp::writer::{document, to_text, EntryInput};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn s(kv: &[(&str, Value)]) -> Settings {
    kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn slots(kinds: &[SlotKind]) -> BTreeMap<SlotKind, SlotFill> {
    kinds
        .iter()
        .map(|k| (*k, SlotFill::Own { item: "x".into() }))
        .collect()
}

fn auto(
    kinds: &[SlotKind],
    own: &Settings,
    tree: Option<bool>,
    path: &str,
) -> Vec<(&'static str, &'static str)> {
    let hint = SsHint {
        tree_anim: tree,
        path: path.into(),
        ..Default::default()
    };
    auto_flags_for(true, &slots(kinds), own, &hint)
}

fn text(own: Settings) -> String {
    let e = EntryInput {
        settings: own,
        slots: BTreeMap::new(),
        derived_match: "landscape\\rock01".into(),
        section: false,
    };
    to_text(&document(&[e]))
}

fn apply(mut own: Settings, mode: SubMode, has_s: bool) -> Settings {
    for (k, v) in sub_edits(mode, has_s) {
        match v {
            Some(v) => own.insert(k.into(), v),
            None => own.remove(k),
        };
    }
    own
}

#[test]
fn the_selector_writes_exactly_one_key_or_none() {
    let none = Settings::new();
    for (mode, has_s, keys) in [
        (SubMode::Standard, false, vec!["subsurface"]),
        (SubMode::Foliage, false, vec!["subsurface_foliage"]),
        (SubMode::Off, false, vec![]),
        (SubMode::Off, true, vec![]),
    ] {
        let t = text(apply(none.clone(), mode, has_s));
        let e = serde_json::from_str::<Value>(&t).unwrap()["entries"][0].clone();
        let found: Vec<&str> = ["subsurface", "subsurface_foliage"]
            .into_iter()
            .filter(|k| e.get(k).is_some())
            .collect();
        assert_eq!(found, keys, "{mode:?}");
        // The writer never writes a false for the foliage key.
        assert!(!t.contains("\"subsurface_foliage\": false"), "{t}");
    }
    // Switching replaces the other key, even from a conflict.
    let both = s(&[
        ("subsurface", json!(true)),
        ("subsurface_foliage", json!(true)),
    ]);
    let f = apply(both.clone(), SubMode::Foliage, false);
    assert_eq!(sub_mode(&f), SubMode::Foliage);
    assert_eq!(
        sub_mode(&apply(both, SubMode::Standard, false)),
        SubMode::Standard
    );
    // Off on a set with an `_s` stores an explicit false so auto stays off.
    let off = apply(none, SubMode::Off, true);
    assert_eq!(off.get("subsurface"), Some(&json!(false)));
    assert!(auto(&[SlotKind::Subsurface], &off, None, "x").is_empty());
}

#[test]
fn a_conflict_loads_as_a_conflict_and_has_its_pitfall() {
    let both = s(&[
        ("subsurface", json!(true)),
        ("subsurface_foliage", json!(true)),
    ]);
    assert_eq!(sub_mode(&both), SubMode::Conflict);
    let w = subsurface_pitfalls(&both, None);
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("both on"));
    // Nothing rewrites it: auto adds nothing on top of it.
    assert!(auto(&[SlotKind::Subsurface], &both, Some(true), "x").is_empty());
}

#[test]
fn the_settings_group_shows_for_standard_and_foliage_only() {
    assert!(subsurface_group_open(&s(&[("subsurface", json!(true))])));
    assert!(subsurface_group_open(&s(&[(
        "subsurface_foliage",
        json!(true)
    )])));
    assert!(!subsurface_group_open(&Settings::new()));
    let hair = s(&[("subsurface", json!(true)), ("hair", json!(true))]);
    assert!(!subsurface_group_open(&hair));
    // Foliage alone writes the colour and opacity.
    let t = text(s(&[
        ("subsurface_foliage", json!(true)),
        ("subsurface_opacity", json!(0.5)),
    ]));
    assert!(t.contains("subsurface_opacity"), "{t}");
}

#[test]
fn auto_picks_foliage_on_tree_anim_or_a_trees_path_else_standard() {
    let none = Settings::new();
    let sub = [SlotKind::Subsurface];
    let a = auto(&sub, &none, Some(true), "landscape\\rock01");
    assert_eq!(
        a,
        [(
            "subsurface_foliage",
            "Foliage: the set's meshes use Tree Anim"
        )]
    );
    let a = auto(&sub, &none, None, "landscape\\trees\\oak");
    assert_eq!(a[0].0, "subsurface_foliage");
    assert!(a[0].1.contains("no scan"), "{a:?}");
    for p in [
        "textures/trees/oak",
        "trees\\oak",
        "landscape\\plants\\fern",
        "plants\\fern",
    ] {
        assert_eq!(auto(&sub, &none, None, p)[0].0, "subsurface_foliage", "{p}");
    }
    let rock = auto(&sub, &none, None, "landscape\\rock01");
    assert_eq!(rock[0].0, "subsurface");
    // A scan with no Tree_Anim mesh beats the path.
    let scanned = auto(&sub, &none, Some(false), "landscape\\trees\\oak");
    assert_eq!(scanned[0].0, "subsurface");
    // Multilayer: `_s` is the coat diffuse.
    let ml = s(&[("multilayer", json!(true))]);
    assert_eq!(auto(&sub, &ml, Some(true), "x")[0].0, "coat_diffuse");
}

#[test]
fn an_explicit_choice_wins_over_auto() {
    let sub = [SlotKind::Subsurface];
    for own in [
        s(&[("subsurface", json!(true))]),
        s(&[("subsurface", json!(false))]),
        s(&[("subsurface_foliage", json!(false))]),
    ] {
        assert!(auto(&sub, &own, Some(true), "x").is_empty(), "{own:?}");
    }
    let hint = SsHint {
        tree_anim: Some(true),
        path: String::new(),
        ..Default::default()
    };
    let own = s(&[("subsurface", json!(true))]);
    let r = resolved_for(true, &slots(&sub), &own, &hint);
    assert_eq!(sub_mode(&r), SubMode::Standard);
}

#[test]
fn the_pitfalls_need_a_scan() {
    let std = s(&[("subsurface", json!(true))]);
    let fol = s(&[("subsurface_foliage", json!(true))]);
    assert!(subsurface_pitfalls(&fol, Some(false))[0].contains("only shades on Tree Anim"));
    assert!(subsurface_pitfalls(&std, Some(true))[0].contains("Foliage is likely meant"));
    for st in [&std, &fol] {
        assert!(subsurface_pitfalls(st, None).is_empty());
    }
    assert!(subsurface_pitfalls(&std, Some(false)).is_empty());
    assert!(subsurface_pitfalls(&fol, Some(true)).is_empty());
}

#[test]
fn the_scan_reads_tree_anim_from_slsf2_bit_29() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    let st = modscan::store::Store::open(&db).unwrap();
    st.conn()
        .execute_batch(
            "INSERT INTO path(id, p) VALUES (1, 'textures/a/tree.dds'), (2, 'textures/a/rock.dds'), (3, 'meshes/a.nif');
             INSERT INTO texset(id, n, t0) VALUES (1, 1, 1), (2, 1, 2);
             INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0), (2, 2, 0);
             INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (1, 1, 'M', 'M', 1, 1);
             INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (1, 3, 1, 0, 0);
             INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES
               (1, 0, 'BSTriShape', 1, 0, 536870912, 1), (1, 1, 'BSTriShape', 1, 0, 0, 2);",
        )
        .unwrap();
    drop(st);
    let keys = ["textures/a/tree", "textures/a/rock", "textures/a/none"]
        .map(String::from)
        .into_iter()
        .collect();
    let r = mesh_tree_anim(&db, &keys).unwrap();
    assert_eq!(r.get("textures/a/tree"), Some(&true));
    assert_eq!(r.get("textures/a/rock"), Some(&false));
    assert_eq!(r.get("textures/a/none"), None);
}

#[test]
fn a_selector_change_is_one_undo_step() {
    use egui_kittest::kittest::Queryable;
    const SET: &str = "landscape/rock01";
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    m.set_json_value(SET, "subsurface_foliage", Some(json!(true)));
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 1100.0))
        .build_ui_state(
            |ui, m: &mut sdc_gui::model::AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("Foliage"))
        .unwrap()
        .click();
    h.run();
    h.get_by_label("Standard").click();
    h.run();
    let has = |h: &egui_kittest::Harness<'_, sdc_gui::model::AppModel>, k: &str| {
        h.state()
            .sets_doc
            .as_ref()
            .unwrap()
            .entries()
            .iter()
            .find(|&e| e.id == SET)
            .cloned()
            .unwrap()
            .settings
            .iter()
            .any(|(n, _)| n == k)
    };
    assert!(has(&h, "subsurface") && !has(&h, "subsurface_foliage"));
    h.state_mut().undo();
    assert!(
        !has(&h, "subsurface") && has(&h, "subsurface_foliage"),
        "one undo restores Foliage"
    );
}

#[test]
fn choosing_the_auto_mode_pins_it() {
    use egui_kittest::kittest::Queryable;
    const SET: &str = "landscape/rock01";
    let d = tempfile::tempdir().unwrap();
    let dir = support::mod_folder(d.path()).join("landscape");
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([200u8, 100, 50]))
        .save(dir.join("rock01_s.png"))
        .unwrap();
    let mut m = support::pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 1100.0))
        .build_ui_state(
            |ui, m: &mut sdc_gui::model::AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("(auto)");
    let stored = |h: &egui_kittest::Harness<'_, sdc_gui::model::AppModel>| {
        h.state()
            .sets_doc
            .as_ref()
            .unwrap()
            .entries()
            .iter()
            .find(|&e| e.id == SET)
            .cloned()
            .into_iter()
            .flat_map(|e| e.settings)
            .find(|(n, _)| n == "subsurface")
            .map(|(_, v)| v)
    };
    assert_eq!(stored(&h), None);
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("Standard"))
        .unwrap()
        .click();
    h.run();
    h.get_by_label("Standard").click();
    h.run();
    assert_eq!(
        stored(&h),
        Some(json!(true)),
        "the auto value is now pinned"
    );
    assert!(h.query_by_label("(auto)").is_none());
}

#[test]
fn foliage_without_an_s_texture_blocks_the_export() {
    use sdc_gui::model::pbr_export::missing_texture_texts;
    let on = s(&[("subsurface_foliage", json!(true))]);
    let t = missing_texture_texts("a/b", &on, |_| false);
    assert_eq!(t.len(), 1);
    assert!(t[0].contains("subsurface foliage"));
    assert!(missing_texture_texts("a/b", &on, |k| k == SlotKind::Subsurface).is_empty());
    // Hair blocks the Subsurface feature, so no texture is asked for.
    let hair = s(&[("subsurface_foliage", json!(true)), ("hair", json!(true))]);
    assert!(missing_texture_texts("a/b", &hair, |_| false).is_empty());
}

/// DECISIONS AS8: on a `pbr_grass` set only the `_s` turns a key on (Standard or Foliage, by main's AG102 rule);
/// `_g`/`_p`/`_cnr`/`_f` would switch on keys grass ignores, and a `_cnr` must not route the `_s` to coat_diffuse.
#[test]
fn a_pbr_grass_set_auto_flags_only_its_subsurface() {
    use SlotKind::*;
    let all = slots(&[Emissive, Height, Coat, Fuzz, Subsurface]);
    let hint = |path: &str| SsHint {
        path: path.into(),
        grass: true,
        ..Default::default()
    };
    let own = Settings::new();
    let keys = |h: &SsHint| -> Vec<&str> {
        auto_flags_for(true, &all, &own, h)
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    };
    assert_eq!(keys(&hint("landscape/grass01")), ["subsurface"]);
    assert_eq!(keys(&hint("plants/fern01")), ["subsurface_foliage"]);
    let r = resolved_for(true, &all, &own, &hint("landscape/grass01"));
    assert_eq!(r, s(&[("subsurface", json!(true))]));
}
