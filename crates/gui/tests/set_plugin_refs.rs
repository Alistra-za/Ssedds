mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use support::*;

/// A taller window: the section sits under the slot cards.
fn tall(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// esp spec §4.1: TXSTs matching the set (EDID, load-order FormID, winning plugin, slot, overridden N×), then the
/// records whose alternate textures use them. Information only. Note FI18 (4, GH7): TXST slot 0 reads "slot 1".
#[test]
fn used_by_plugins_lists_txsts_and_alternate_texture_users() {
    use modscan::plugin_store::{AltUse, ShapeSwap, TxstUse};
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .expect("a set with a match key");
    let key = set_key(&v).unwrap();
    model.plugin_refs.cached = Some((
        key,
        Ok(SetPluginUse {
            txsts: vec![TxstUse {
                formid: 0x0001_2345,
                edid: Some("RockTXST".into()),
                plugin: "Skyrim.esm".into(),
                slot: 0,
                path: "textures/landscape/rock01.dds".into(),
                n_overrides: 1,
            }],
            alts: vec![AltUse {
                formid: 0x0000_0DEF,
                edid: Some("RockStat".into()),
                sig: "STAT".into(),
                plugin: "Skyrim.esm".into(),
                model_slot: 0,
                swap: ShapeSwap {
                    shape_name: "Shape0".into(),
                    txst_formid: 0x0001_2345,
                    txst_edid: Some("RockTXST".into()),
                    index: 0,
                },
            }],
            truncated: false,
            inert: Default::default(),
        }),
    ));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 1: textures/landscape/rock01.dds · overridden 1×");
    h.get_by_label("RockStat [00000DEF] STAT — Skyrim.esm · Shape0 → RockTXST");
}

#[test]
fn used_by_plugins_without_a_scan_says_so() {
    use sdc_gui::model::plugin_refs::{set_key, NO_SCAN};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .unwrap();
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label(NO_SCAN);
}

#[test]
fn used_by_plugins_says_when_nothing_matches() {
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .unwrap();
    model.plugin_refs.cached = Some((set_key(&v).unwrap(), Ok(SetPluginUse::default())));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("No texture set of the latest scan matches this set.");
}

/// Ruling R9: the set's own match key, with the derived `match_diffuse` when it names none.
#[test]
fn the_set_key_is_pgpatchers_match_rule() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::set_key;
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let model = pbr_opened(d.path());
    let mut v = set_views(&model).into_iter().next().unwrap();
    v.settings.clear();
    v.derived_match = Some(r"landscape\rock01.dds".into());
    assert_eq!(
        set_key(&v),
        Some(TexKey::Suffix {
            slot: 0,
            stem: r"landscape\rock01".into()
        })
    );
    v.settings.insert(
        "match_normal".into(),
        serde_json::json!(r"landscape\rock01_n.dds"),
    );
    assert_eq!(
        set_key(&v),
        Some(TexKey::Suffix {
            slot: 1,
            stem: r"landscape\rock01_n".into()
        })
    );
    v.settings.clear();
    v.derived_match = None;
    assert_eq!(set_key(&v), None);
}

/// A scan database with one winning TXST (`RockTXST`) on `textures/landscape/rock01.dds`, slot 0.
fn seeded_db(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("scan.db");
    drop(modscan::store::Store::open(&p).unwrap());
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch(
        "INSERT INTO plugin(id, name, path, mtime, light, flags, masters_json) VALUES (1, 'Skyrim.esm', 'x', 0, 0, 0, '[]');
         INSERT INTO path(id, p) VALUES (1, 'textures/landscape/rock01.dds');",
    )
    .unwrap();
    add_txst(&p, 0x12345, "RockTXST");
    p
}

fn add_txst(db: &std::path::Path, formid: u32, edid: &str) {
    rusqlite::Connection::open(db)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO rec(formid, local, plugin_id, sig, edid, owner_ok) VALUES ({formid}, {formid}, 1, 'TXST', '{edid}', 1);
             INSERT INTO rec_winner(formid, plugin_id, n_overrides) VALUES ({formid}, 1, 1);
             INSERT INTO txst_slot(formid, local, plugin_id, slot, path_id) VALUES ({formid}, {formid}, 1, 0, 1);"
        ))
        .unwrap();
}

/// The panel reads the model's `scan_db` when opened and again on Refresh.
#[test]
fn used_by_plugins_reads_the_scan_db_and_refresh_rereads_it() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::set_key;
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let db = seeded_db(d.path());
    let mut model = pbr_opened_plain(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(
            |v| matches!(set_key(v), Some(TexKey::Suffix { stem, .. }) if stem.ends_with("rock01")),
        )
        .unwrap();
    model.scan_db = Some(db.clone());
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 1: textures/landscape/rock01.dds · overridden 1×");
    assert!(h.query_by_label_contains("Rock2TXST").is_none());
    add_txst(&db, 0x2000, "Rock2TXST");
    h.run();
    assert!(
        h.query_by_label_contains("Rock2TXST").is_none(),
        "cached until Refresh"
    );
    h.get_by_label("Refresh").click();
    h.run();
    h.get_by_label_contains("Rock2TXST [00002000] TXST");
}

#[test]
fn plugin_refs_requery_on_a_new_key_and_report_db_problems() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::{SetPluginRefs, NO_SCAN};
    let d = tempfile::tempdir().unwrap();
    let db = seeded_db(d.path());
    let rock = TexKey::Suffix {
        slot: 0,
        stem: r"landscape\rock01".into(),
    };
    let other = TexKey::Suffix {
        slot: 0,
        stem: r"landscape\other".into(),
    };
    let mut r = SetPluginRefs::default();
    assert_eq!(r.get(Some(&db), &rock).as_ref().unwrap().txsts.len(), 1);
    assert!(r.get(Some(&db), &other).as_ref().unwrap().txsts.is_empty());
    assert_eq!(r.cached.as_ref().unwrap().0, other);
    assert_eq!(r.get(Some(&db), &rock).as_ref().unwrap().txsts.len(), 1);

    let mut r = SetPluginRefs::default();
    let missing = d.path().join("nope.db");
    assert_eq!(r.get(Some(&missing), &rock).as_ref().unwrap_err(), NO_SCAN);
    assert_eq!(r.get(None, &rock).as_ref().unwrap_err(), NO_SCAN);

    let old = d.path().join("old.db");
    rusqlite::Connection::open(&old)
        .unwrap()
        .execute_batch("PRAGMA user_version = 3")
        .unwrap();
    let mut r = SetPluginRefs::default();
    let e = r.get(Some(&old), &rock).as_ref().unwrap_err();
    assert!(e.contains("scan database version 3"), "{e}");
}

/// Review M3: an Err (no scan yet) is not cached, so a scan that lands afterwards shows without Refresh.
#[test]
fn an_error_result_is_not_cached() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::{SetPluginRefs, NO_SCAN};
    let d = tempfile::tempdir().unwrap();
    let key = TexKey::Suffix {
        slot: 0,
        stem: "landscape/rock01".into(),
    };
    let db = d.path().join("scan.db");
    let mut refs = SetPluginRefs::default();
    assert_eq!(refs.get(Some(&db), &key).as_ref().unwrap_err(), NO_SCAN);
    seeded_db(d.path());
    let r = refs.get(Some(&db), &key).as_ref().unwrap();
    assert_eq!(r.txsts.len(), 1);
}

/// Filtering spec §3 / ruling R12: an inert alternate-texture user is hidden until "Show inert (N)", then dimmed with
/// its reason.
#[test]
fn used_by_plugins_hides_inert_records_until_asked() {
    use modscan::plugin_store::{AltUse, ShapeSwap, TxstUse};
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .unwrap();
    model.plugin_refs.cached = Some((
        set_key(&v).unwrap(),
        Ok(SetPluginUse {
            txsts: vec![TxstUse {
                formid: 0x0001_2345,
                edid: Some("RockTXST".into()),
                plugin: "Skyrim.esm".into(),
                slot: 0,
                path: "textures/landscape/rock01.dds".into(),
                n_overrides: 1,
            }],
            alts: vec![AltUse {
                formid: 0x0000_0DEF,
                edid: Some("RockStat".into()),
                sig: "STAT".into(),
                plugin: "Skyrim.esm".into(),
                model_slot: 0,
                swap: ShapeSwap {
                    shape_name: "Shape0".into(),
                    txst_formid: 0x0001_2345,
                    txst_edid: Some("RockTXST".into()),
                    index: 0,
                },
            }],
            truncated: false,
            inert: [(0x0000_0DEFu32, "not placed")].into_iter().collect(),
        }),
    ));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 1: textures/landscape/rock01.dds · overridden 1×");
    assert!(h.query_by_label_contains("RockStat").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    h.get_by_label("RockStat [00000DEF] STAT — Skyrim.esm · Shape0 → RockTXST · not placed");
}

/// Task 14 lesson: "Show inert" never leaks across a key change or a Refresh.
#[test]
fn show_inert_resets_on_key_change_and_refresh() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::SetPluginRefs;
    let d = tempfile::tempdir().unwrap();
    let db = seeded_db(d.path());
    let key = |s: &str| TexKey::Suffix {
        slot: 0,
        stem: s.into(),
    };
    let mut r = SetPluginRefs::default();
    r.get(Some(&db), &key(r"landscape\rock01"))
        .as_ref()
        .unwrap();
    r.show_inert = true;
    r.get(Some(&db), &key(r"landscape\rock01"))
        .as_ref()
        .unwrap();
    assert!(r.show_inert, "same key, cached: kept");
    r.get(Some(&db), &key(r"landscape\other")).as_ref().unwrap();
    assert!(!r.show_inert, "key change resets");
    r.show_inert = true;
    r.refresh();
    assert!(!r.show_inert, "Refresh resets");
}

/// Ruling F-21: with every TXST inert and the toggle off, the empty message shows; the toggle brings the rows back.
#[test]
fn used_by_plugins_all_inert_says_nothing_matches_until_shown() {
    use modscan::plugin_store::TxstUse;
    use sdc_gui::model::plugin_refs::{set_key, SetPluginUse};
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let mut model = pbr_opened(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(|v| set_key(v).is_some())
        .unwrap();
    model.plugin_refs.cached = Some((
        set_key(&v).unwrap(),
        Ok(SetPluginUse {
            txsts: vec![TxstUse {
                formid: 0x0001_2345,
                edid: Some("RockTXST".into()),
                plugin: "Skyrim.esm".into(),
                slot: 0,
                path: "textures/landscape/rock01.dds".into(),
                n_overrides: 1,
            }],
            inert: [(0x0001_2345u32, "no reference")].into_iter().collect(),
            ..Default::default()
        }),
    ));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("No texture set of the latest scan matches this set.");
    assert!(h.query_by_label_contains("RockTXST").is_none());
    h.get_by_label("Show inert (1)").click();
    h.run();
    assert!(h
        .query_by_label("No texture set of the latest scan matches this set.")
        .is_none());
    h.get_by_label_contains("RockTXST [00012345] TXST — Skyrim.esm · slot 1: textures/landscape/rock01.dds · overridden 1× · no reference");
}

/// Filtering spec §3 through the real `get` (so `inert_of` is exercised): an inert TXST and an inert alternate-texture
/// user are hidden by default, counted, and shown dimmed with their reasons when asked.
#[test]
fn used_by_plugins_reads_inert_from_the_scan_db() {
    use modscan::store::TexKey;
    use sdc_gui::model::plugin_refs::set_key;
    use sdc_gui::model::sets::set_views;
    let d = tempfile::tempdir().unwrap();
    let db = seeded_db(d.path());
    add_txst(&db, 0x2000, "LiveTXST");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO rec(formid, local, plugin_id, sig, edid, owner_ok) VALUES (3567, 3567, 1, 'STAT', 'DeadStat', 1);
             INSERT INTO rec_winner(formid, plugin_id, n_overrides) VALUES (3567, 1, 0);
             INSERT INTO alt_tex(formid, local, plugin_id, model_slot, shape_name, txst_formid, txst_local, idx)
                 VALUES (3567, 3567, 1, 0, 'Shape0', 8192, 8192, 0);
             INSERT INTO rec_inert(formid, reason) VALUES (74565, 1), (3567, 2);",
        )
        .unwrap();
    let mut model = pbr_opened_plain(d.path());
    let v = set_views(&model)
        .into_iter()
        .find(
            |v| matches!(set_key(v), Some(TexKey::Suffix { stem, .. }) if stem.ends_with("rock01")),
        )
        .unwrap();
    model.scan_db = Some(db);
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label_contains("LiveTXST [00002000] TXST");
    assert!(h.query_by_label_contains("RockTXST").is_none());
    assert!(h.query_by_label_contains("DeadStat").is_none());
    h.get_by_label("Show inert (2)").click();
    h.run();
    h.get_by_label_contains("RockTXST [00012345] TXST — Skyrim.esm · slot 1: textures/landscape/rock01.dds · overridden 1× · no reference");
    h.get_by_label("DeadStat [00000DEF] STAT — Skyrim.esm · Shape0 → LiveTXST · not placed");
}
