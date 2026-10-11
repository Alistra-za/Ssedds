mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use support::*;

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_sets_column_shows_badges_and_the_new_set_area() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    h.run();
    assert!(
        h.query_by_label("Defaults").is_none(),
        "the default block is fixed: no Defaults card"
    );
    h.get_by_label("landscape/rock01");
    h.get_by_label_contains("PBR → JSON");
    h.get_by_label_contains("Drop here to start a new set");
}

#[test]
fn the_textures_header_offers_the_not_in_a_set_filter() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label_contains("not in a set");
}
