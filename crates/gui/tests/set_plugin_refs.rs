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
/// records whose alternate textures use them. Information only.
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
        }),
    ));
    model.select_set(&v.key);
    let mut h = tall(model);
    h.run();
    h.get_by_label("Used by plugins").click();
    h.run();
    h.get_by_label("RockTXST [00012345] TXST — Skyrim.esm · slot 0: textures/landscape/rock01.dds · overridden 1×");
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
    v.derived_match = Some("landscape\rock01.dds".into());
    assert_eq!(
        set_key(&v),
        Some(TexKey::Suffix {
            slot: 0,
            stem: "landscape\rock01".into()
        })
    );
    v.settings.insert(
        "match_normal".into(),
        serde_json::json!("landscape\rock01_n.dds"),
    );
    assert_eq!(
        set_key(&v),
        Some(TexKey::Suffix {
            slot: 1,
            stem: "landscape\rock01_n".into()
        })
    );
    v.settings.clear();
    v.derived_match = None;
    assert_eq!(set_key(&v), None);
}
