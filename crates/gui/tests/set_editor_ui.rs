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

#[test]
fn the_rename_dialog_shows_a_collision_and_renames_on_a_free_name() {
    use sdc_gui::model::sets::set_views;
    use sdc_gui::ui::sets_column::Pending;
    use sdc_gui::ui::State;
    let d = tempfile::tempdir().unwrap();
    let model = pbr_opened(d.path());
    let ids: Vec<String> = set_views(&model).into_iter().map(|v| v.id).collect();
    assert!(ids.len() >= 2, "the fixture has two sets: {ids:?}");
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, (model, state): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, model, state),
            (model, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.state_mut().1.drops.pending = Some(Pending::Rename {
        id: ids[0].clone(),
        text: String::new(),
        error: None,
    });
    h.run();
    h.get_by_label_contains("Rename ");
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .last()
        .expect("the rename field")
        .type_text(&ids[1]);
    h.run();
    h.get_by_label("OK").click();
    h.run();
    h.get_by_label_contains("already");
    // A free name renames and closes the dialog.
    if let Some(Pending::Rename { text, .. }) = &mut h.state_mut().1.drops.pending {
        text.clear();
    }
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .last()
        .expect("the rename field")
        .type_text("renamed-set");
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert!(h.query_by_label_contains("Rename ").is_none());
    let after: Vec<String> = set_views(&h.state().0).into_iter().map(|v| v.id).collect();
    assert!(after.contains(&"renamed-set".to_string()), "{after:?}");
}
