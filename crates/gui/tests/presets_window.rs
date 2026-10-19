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
    // R14, K5: Presets… ends the Preset popup, which opens with no folder too.
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| {
            n.accesskit_node()
                .value()
                .or(n.accesskit_node().label())
                .is_some_and(|v| v.starts_with("default ("))
        })
        .expect("the Preset combo")
        .click();
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
    collapse_pbr(&mut h);
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

fn with_folder(dir: &std::path::Path) -> AppModel {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.user_presets = Some(dir.to_path_buf());
    m.open_presets();
    m
}

#[test]
fn unsaved_edits_ask_before_new_and_esc_keeps_them() {
    use engine::config::Subtype;
    let d = tempfile::tempdir().unwrap();
    let mut m = with_folder(d.path());
    m.presets_model().select("cs-quality");
    m.presets_model().duplicate();
    m.presets_model().set_id("edited");
    m.presets_model()
        .set_field(Subtype::Diffuse, "resize", serde_json::json!("half"));
    let mut h = harness(m);
    h.run();
    h.get_by_label("New").click();
    h.run();
    h.get_by_label("Discard unsaved changes to edited?");
    // A7 / R18: the discard question has no Enter default; Enter keeps it open and the edits.
    h.run();
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    h.get_by_label("Discard unsaved changes to edited?");
    assert_eq!(
        h.state_mut().presets_model().id(),
        "edited",
        "Enter kept the edits"
    );
    h.key_press(eframe::egui::Key::Escape);
    h.run();
    h.run();
    assert!(h
        .query_by_label("Discard unsaved changes to edited?")
        .is_none());
    assert_eq!(
        h.state_mut().presets_model().id(),
        "edited",
        "Esc kept the edits"
    );
    h.get_by_label("New").click();
    h.run();
    h.get_by_label("Discard").click();
    h.run();
    assert_eq!(h.state_mut().presets_model().id(), "", "a new preset");
}

#[test]
fn delete_names_the_file_and_broken_files_are_listed_with_open_folder() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("broken.toml"), "id = ").unwrap();
    std::fs::write(
        d.path().join("other-name.toml"),
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    let mut m = with_folder(d.path());
    m.presets_model().select("mine");
    let mut h = harness(m);
    h.run();
    h.get_by_label("1 preset files did not load:");
    h.get_by_label_contains("broken.toml: ");
    h.get_by_label("Open folder").click();
    h.run();
    assert_eq!(
        h.state_mut().take_shell(),
        [sdc_gui::model::shell::ShellRequest::Open(
            d.path().to_path_buf()
        )]
    );
    h.get_by_label("Delete…").click();
    h.run();
    h.get_by_label_contains("other-name.toml?");
}

#[test]
fn the_orm_example_is_a_read_only_branch_under_rmaos() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("rmaos —").click();
    h.run();
    h.get_by_label_contains("_orm → _rmaos — ORM → RMAOS");
    assert_eq!(h.query_all_by_label_contains("_rmaos — ").count(), 2);
}

#[test]
fn a_branch_shows_its_overrides_hints_the_sub_type_and_resets() {
    use engine::config::Subtype;
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    m.presets_model().duplicate();
    m.presets_model()
        .set_branch_field(Subtype::Diffuse, "d", "resize", serde_json::json!("half"));
    let mut h = harness(m);
    h.run();
    collapse_pbr(&mut h);
    h.get_by_label("diffuse — BC7 · linear").click();
    h.run();
    h.get_by_label("_d — BC7 · linear (overrides 1)").click();
    h.run();
    h.get_by_label_contains("inherits: None");
    h.get_by_label("↺").click();
    h.run();
    assert_eq!(
        h.state_mut()
            .presets_model()
            .branch_field(Subtype::Diffuse, "d", "resize"),
        None
    );
    assert!(!h.state_mut().presets_model().toml_text().contains("rules"));
}

#[test]
fn add_alias_opens_its_empty_branch() {
    const TEXT: eframe::egui::accesskit::Role = eframe::egui::accesskit::Role::TextInput;
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    m.presets_model().duplicate();
    let mut h = harness(m);
    h.run();
    collapse_pbr(&mut h);
    h.get_by_label("diffuse — BC7 · linear").click();
    h.run();
    h.get_by_role_and_label(TEXT, "New alias _").focus();
    h.run();
    h.get_by_role_and_label(TEXT, "New alias _")
        .type_text("foo");
    h.run();
    h.get_by_label("Add alias").click();
    h.run();
    h.run();
    h.get_by_label("_foo → _d — BC7 · linear");
    h.get_by_label("Remove alias");
}

#[test]
fn the_families_show_in_order_with_their_hints() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    m.presets_model().select("cs-quality");
    let h = harness(m);
    let y = |label: &str| h.get_by_label(label).rect().top();
    let (pbr, van) = (y("PBR / Community Shaders"), y("Vanilla"));
    assert!(pbr < van, "{pbr} {van}");
    h.get_by_label("PBR / Community Shaders — used under the cs target");
    // pbr_emissive under PBR / CS; glow and complex material under Vanilla (Note AZ).
    let emissive = h.get_by_label_contains("emissive —").rect().top();
    let glow = h.get_by_label_contains("glow —").rect().top();
    let complex = h.get_by_label_contains("complex material —").rect().top();
    assert!(pbr < emissive && emissive < van, "{emissive}");
    assert!(van < glow && van < complex, "{glow} {complex}");
}

/// The PBR / CS family comes first and is long: fold it so the Vanilla rows are on screen.
fn collapse_pbr(h: &mut Harness<'static, AppModel>) {
    h.get_by_label("PBR / Community Shaders").click();
    h.run();
}
