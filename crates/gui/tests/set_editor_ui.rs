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

#[test]
fn the_set_editor_shows_slot_cards_opaque_and_slot_commands() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    for label in [
        "Diffuse",
        "Normal _n",
        "RMAOS",
        "Height _p",
        "Emissive _g",
        "Subsurface _s",
        "Coat _cnr",
        "Fuzz _f",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label("Opaque");
    // A combo box exposes its text as a value, not a label.
    assert!(
        h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
            .any(|n| n.value().as_deref() == Some("BC7 · sRGB")),
        "the diffuse card shows its preset"
    );
    h.get_by_label("Slot commands…").click();
    h.run();
    h.get_by_label_contains("Slot commands: landscape/rock01");
    assert!(
        h.query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
            .any(|n| n.value().as_deref() == Some("From this set")),
        "every slot starts as From this set"
    );
}

#[test]
fn opaque_and_format_edit_the_diffuse_in_one_step_each() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let diffuse = m
        .items
        .iter()
        .find(|i| i.name == "rock01.png")
        .unwrap()
        .key
        .clone();
    m.set_opaque(&diffuse, true);
    assert!(m.is_opaque(&diffuse));
    m.undo();
    assert!(!m.is_opaque(&diffuse));
    m.set_format(
        &diffuse,
        engine::config::Format::Bc1,
        engine::config::ColorTag::Srgb,
    );
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.file_value(&diffuse, "format"),
        Some(serde_json::json!("bc1"))
    );
    assert_eq!(
        p.file_value(&diffuse, "color.tag"),
        Some(serde_json::json!("srgb"))
    );
    m.undo();
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.file_value(&diffuse, "format"),
        None,
        "both fields undo together"
    );
    assert_eq!(p.file_value(&diffuse, "color.tag"), None);
}

#[test]
fn a_bad_slot_path_is_refused_with_the_reason() {
    use sdc_gui::model::sets::SlotCommand;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert!(m
        .set_slot_command("landscape/rock01", 3, SlotCommand::Path(r"..\x.dds".into()))
        .is_err());
    assert!(m
        .set_slot_command(
            "landscape/rock01",
            3,
            SlotCommand::Path(r"effects\white.png".into())
        )
        .is_err());
    m.set_slot_command(
        "landscape/rock01",
        3,
        SlotCommand::Path(r"effects\white.dds".into()),
    )
    .unwrap();
    m.set_slot_command("landscape/rock01", 5, SlotCommand::Clear)
        .unwrap();
    let e = m
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .into_iter()
        .find(|e| e.id == "landscape/rock01")
        .unwrap();
    assert_eq!(e.settings["slot3"], serde_json::json!(r"effects\white.dds"));
    assert_eq!(e.settings["slot5"], serde_json::json!(""));
}

#[test]
fn a_member_links_back_to_its_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let diffuse = m
        .items
        .iter()
        .find(|i| i.name == "rock01.png")
        .unwrap()
        .key
        .clone();
    m.select_item(&diffuse);
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("← ").click();
    h.run();
    assert!(matches!(
        h.state().selection,
        sdc_gui::model::Selection::Set(_)
    ));
}
