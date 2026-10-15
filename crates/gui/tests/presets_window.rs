//! Note AM: the preset creator window (kittest). No presets folder: nothing can be written.
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1000.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn presets_opens_the_creator_and_a_builtin_is_read_only() {
    let mut h = harness(AppModel::placeholder(GuiSettings::default()));
    h.run();
    h.get_by_label("Presets…").click();
    h.run();
    assert!(h.state().presets_open);
    h.state_mut().presets_model().select("cs-quality");
    h.run();
    h.get_by_label_contains("Built-in presets are read-only");
    assert!(h.get_by_label("Save preset").accesskit_node().is_disabled());
    h.get_by_label("Duplicate").click();
    h.run();
    assert_eq!(h.state_mut().presets_model().id(), "cs-quality-copy");
    h.query_by_label_contains("Built-in presets are read-only")
        .is_none()
        .then_some(())
        .expect("the copy is editable");
}

#[test]
fn the_orm_example_is_shown_read_only_and_never_written() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    m.presets_model().duplicate();
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("rmaos —").click();
    h.run();
    h.get_by_label_contains("ORM → RMAOS");
    assert!(!h.state_mut().presets_model().toml_text().contains("orm"));
}

#[test]
fn the_reset_arrow_drops_an_override() {
    use engine::config::Subtype;
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    m.presets_model().duplicate();
    m.presets_model()
        .set_field(Subtype::Diffuse, "resize", serde_json::json!("half"));
    let mut h = harness(m);
    h.run();
    h.get_by_label("diffuse — BC7 · linear").click();
    h.run();
    h.get_by_label("Suffixes");
    // Colour rows have no Channel remap or Packing fills.
    assert!(h.query_by_label("Packing fills").is_none());
    h.get_by_label("Alpha");
    h.get_by_label_contains("base: None");
    h.get_by_label("↺").click();
    h.run();
    assert_eq!(
        h.state_mut()
            .presets_model()
            .field(Subtype::Diffuse, "resize"),
        None
    );
    assert!(h.query_by_label("↺").is_none());
}
