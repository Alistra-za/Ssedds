//! Note AM: the preset creator, a centre tab since Note FB7 (kittest). No presets folder: nothing can be written.
mod support;
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
    assert!(
        h.query_by_label("Save preset").is_none(),
        "a built-in offers only Duplicate"
    );
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

/// Trace #22: the Delete confirmation belongs to the preset it was asked on, not to the next one picked.
#[test]
fn the_delete_question_is_per_preset() {
    let d = tempfile::tempdir().unwrap();
    for id in ["aa", "bb"] {
        std::fs::write(
            d.path().join(format!("{id}.toml")),
            format!(
                "schema_version = 1\nid = \"{id}\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n"
            ),
        )
        .unwrap();
    }
    let mut m = with_folder(d.path());
    m.presets_model().select("aa");
    let mut h = harness(m);
    h.run();
    h.get_by_label("Delete…").click();
    h.run();
    h.get_by_label_contains("aa.toml?");
    h.state_mut().presets_model().select("bb");
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("bb.toml?").is_none(),
        "armed for bb"
    );
    h.get_by_label("Delete…");
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

/// Note F?16: the Presets tab's project Mode row (the combo moved out of the header).
fn mode_name(target: &str) -> String {
    format!("Mode (this project): {target}")
}

fn preset_combo_text(h: &Harness<'static, AppModel>) -> String {
    h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .filter_map(|n| {
            n.accesskit_node()
                .value()
                .or(n.accesskit_node().label())
                .map(|v| v.to_string())
        })
        .find(|v| v.starts_with("default ("))
        .expect("the Preset combo")
}

#[test]
fn the_presets_tab_sets_the_project_mode() {
    use engine::config::Target;
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(support::pbr_opened(d.path()));
    h.run();
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
    h.run();
    h.state_mut().presets_model().select("cs-quality");
    h.run();
    h.get_by_label(&mode_name("cs")).click();
    h.run();
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Button, "vanilla")
        .click();
    h.run();
    // The target the plan reports lands with the re-plan.
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    h.run();
    assert_eq!(h.state().target, Target::Vanilla);
    let text = h.state().project.as_ref().unwrap().text();
    assert!(text.contains("target = \"vanilla\""), "{text}");
    assert_eq!(h.state().undo_label(), Some("Mode"));
    // Review Focus 7: the preset draft is untouched.
    assert!(!h.state_mut().presets_model().dirty());
    h.get_by_label(&mode_name("vanilla"));
    let want = format!(
        "default ({})",
        engine::presets::default_preset_for(Target::Vanilla)
    );
    assert_eq!(preset_combo_text(&h), want);
    h.state_mut().undo();
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    h.run();
    assert_eq!(h.state().target, Target::Cs);
    h.get_by_label(&mode_name("cs"));
}

#[test]
fn the_mode_row_is_off_with_no_folder() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.open_presets();
    let mut h = harness(m);
    h.run();
    let name = mode_name("cs");
    let combo = h.get_by_label(&name);
    assert!(combo.accesskit_node().is_disabled());
    combo.hover();
    h.run();
    h.get_by_label_contains("Open a folder first");
}

#[test]
fn the_mode_row_is_off_when_read_only() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    std::fs::write(d.path().join("textures/sdc.toml"), "this is = [not toml").unwrap();
    m.reload_documents();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(m.project.as_ref().unwrap().read_only());
    m.open_presets();
    let mut h = harness(m);
    h.run();
    let name = mode_name("cs");
    let combo = h.get_by_label(&name);
    assert!(combo.accesskit_node().is_disabled());
    combo.hover();
    h.run();
    h.get_by_label_contains("sdc.toml is read-only");
    assert_eq!(
        std::fs::read_to_string(d.path().join("textures/sdc.toml")).unwrap(),
        "this is = [not toml"
    );
}

#[test]
fn the_mode_row_shows_in_the_popped_out_window() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.open_presets();
    m.presets_popped = true;
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label(&mode_name("cs"));
    h.get_by_label("This project (sdc.toml)");
}

/// The plan's 1280 px capture of the Presets tab, as a test: the Mode row is there and its pieces do not overlap.
#[test]
fn the_mode_row_fits_at_1280() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    m.open_presets();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let heading = h.get_by_label("This project (sdc.toml)").rect();
    let label = h.get_by_label("Mode").rect();
    let combo = h.get_by_label(&mode_name("cs")).rect();
    let note = h.get_by_label("Wins over a preset's own Target.").rect();
    let all = [heading, label, combo, note];
    for (i, a) in all.iter().enumerate() {
        assert!(a.right() <= 1280.5, "{a:?} runs off the window");
        for b in &all[i + 1..] {
            assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "{a:?} {b:?}");
        }
    }
}

/// The PBR / CS family comes first and is long: fold it so the Vanilla rows are on screen.
fn collapse_pbr(h: &mut Harness<'static, AppModel>) {
    h.get_by_label("PBR / Community Shaders").click();
    h.run();
}
