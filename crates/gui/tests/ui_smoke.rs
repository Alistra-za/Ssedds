use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;

#[test]
fn the_empty_window_shows_its_regions() {
    let mut h = Harness::new_ui_state(
        |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
        AppModel::placeholder(GuiSettings::default()),
    );
    // No bundled fonts: the harness needs the system fonts, as the app does.
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    for label in [
        "Open folder…",
        "Mode",
        "Export",
        "Textures",
        "Sets",
        "Advanced",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("Open a folder");
}
