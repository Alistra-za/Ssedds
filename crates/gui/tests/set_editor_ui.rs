mod support;
use egui_kittest::kittest::{NodeT, Queryable};
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
    // Tall enough for the packed card's recipe choice and fill rows, which push the commands button down.
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1200.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    for label in [
        "Diffuse",
        "Normal _n",
        "RMAOS",
        "Height _p",
        "Emissive _g",
        "Skin _sk",
        "Subsurface _s",
        "Coat _cnr",
        "Fuzz _f",
    ] {
        // The Matching header repeats the Diffuse/Normal slot names as its row labels.
        assert!(h.query_all_by_label(label).next().is_some(), "{label}");
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

fn rock_selected(m: &mut AppModel) {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
}

fn rock_settings(
    h: &Harness<'static, AppModel>,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    h.state()
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .into_iter()
        .find(|e| e.id == "landscape/rock01")
        .map(|e| e.settings.into_iter().collect())
        .unwrap_or_default() // no entry yet = nothing written
}

#[test]
fn feature_toggles_grey_their_exclusions_and_reveal_their_groups() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Specular level");
    assert!(
        h.query_by_label("Displacement scale").is_none(),
        "parallax group hidden while off"
    );
    h.get_by_label("Parallax").click();
    h.run();
    h.get_by_label("Displacement scale");
    h.get_by_label("Hair").click();
    h.run();
    let s = rock_settings(&h);
    assert_eq!(s["parallax"], serde_json::json!(true));
    assert!(
        !s.contains_key("hair"),
        "hair is greyed while parallax is on"
    );
}

fn spin_values(h: &Harness<'static, AppModel>) -> Vec<String> {
    h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .filter_map(|n| n.value())
        .collect()
}

#[test]
fn an_unset_number_shows_the_fixed_default_and_an_edit_is_the_sets_own() {
    use sdc_gui::pgp::settings::GLINT_DEFAULTS;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(
        h.query_by_label("Defaults").is_none(),
        "there is no Defaults editor"
    );
    let v = spin_values(&h);
    assert!(v.contains(&"0.04".to_string()), "specular level: {v:?}");
    assert!(v.contains(&"1.0".to_string()), "roughness scale: {v:?}");
    assert!(
        h.query_by_label("reset").is_none(),
        "no reset while nothing is the set's own"
    );
    // An edit is the set's own: the reset button appears, and clicking it removes the key.
    h.state_mut().set_json_value(
        "landscape/rock01",
        "roughness_scale",
        Some(serde_json::json!(0.5)),
    );
    h.run();
    assert!(spin_values(&h).contains(&"0.5".to_string()));
    assert_eq!(h.query_all_by_label("reset").count(), 1);
    h.get_by_label("reset").click();
    h.run();
    assert!(!rock_settings(&h).contains_key("roughness_scale"));
    assert!(h.query_by_label("reset").is_none());
    // The dependent groups show their defaults too: displacement scale, and the four glint values.
    let mut m = pbr_opened(tempfile::tempdir().unwrap().path());
    rock_selected(&mut m);
    m.set_json_value(
        "landscape/rock01",
        "parallax",
        Some(serde_json::json!(true)),
    );
    let mut h = harness(m);
    h.run();
    assert!(
        spin_values(&h).contains(&"1.0".to_string()),
        "displacement scale"
    );
    h.get_by_label("Parallax").click();
    h.run();
    h.get_by_label("Glint").click();
    h.run();
    let v = spin_values(&h);
    for (_, x) in GLINT_DEFAULTS {
        let t = format!("{x:?}");
        assert!(v.contains(&t), "glint default {t} in {v:?}");
    }
}

/// The PGPatcher panel of `landscape/rock01` alone.
fn panel_harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, "landscape/rock01"),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Focus the number box showing `shown` and select its text (egui_kittest's DragValue pattern). If the accesskit
/// `focus()` does not put the DragValue into text editing, replace it with `.click()`: a plain click on a DragValue
/// requests focus.
fn focus_spin(h: &mut Harness<'static, AppModel>, shown: &str) {
    h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap_or_else(|| panic!("no number box shows {shown}: {:?}", spin_values(h)))
        .focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.run();
}

/// Type one character into the focused widget. Text events go to the focused widget, whichever node sends them.
fn type_char(h: &Harness<'static, AppModel>, c: char) {
    h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .next()
        .expect("a number box")
        .type_text(&c.to_string());
}

/// F1 spec §2: egui clamps a drag to the range the box reports, so the range is the drag test.
#[test]
fn every_number_box_reports_its_range() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "multilayer",
        Some(serde_json::json!(true)),
    );
    m.set_json_value(
        "landscape/rock01",
        "coat_strength",
        Some(serde_json::json!(0.5)),
    );
    let mut h = panel_harness(m);
    h.run();
    let spins: Vec<_> = h
        .query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .map(|n| {
            let a = n.accesskit_node();
            (n.value(), a.min_numeric_value(), a.max_numeric_value())
        })
        .collect();
    assert!(spins.len() >= 5, "{spins:?}");
    for (v, min, max) in &spins {
        assert_eq!(*min, Some(0.0), "{v:?}");
        let unit = v.as_deref() == Some("0.5");
        assert_eq!(*max, unit.then_some(1.0), "{v:?}");
    }
}

/// F1 spec §2: a typed value outside the range is clamped, and the box says so for that frame.
#[test]
fn typed_values_clamp_and_say_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "multilayer",
        Some(serde_json::json!(true)),
    );
    let mut h = panel_harness(m);
    h.run();
    // roughness_scale shows its fixed default 1.0.
    focus_spin(&mut h, "1.0");
    type_char(&h, '-');
    h.run();
    type_char(&h, '5');
    h.step();
    h.get_by_label("clamped to ≥ 0");
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert_eq!(rock_settings(&h)["roughness_scale"], serde_json::json!(0.0));
    // coat_strength has no default ("not set"): a 2 becomes 1.
    h.state_mut().set_json_value(
        "landscape/rock01",
        "coat_strength",
        Some(serde_json::json!(0.5)),
    );
    h.run();
    focus_spin(&mut h, "0.5");
    type_char(&h, '2');
    h.step();
    h.get_by_label("clamped to 0–1");
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert_eq!(rock_settings(&h)["coat_strength"], serde_json::json!(1.0));
}

/// F1 spec Review Focus 3: a hand-edited value outside the range shows as it is with the note. Drawing it is no
/// edit, so the project stays clean (the writer clamps it on export).
#[test]
fn a_hand_edited_negative_number_shows_as_it_is_and_does_not_dirty_the_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "roughness_scale",
        Some(serde_json::json!(-3.0)),
    );
    m.save().unwrap();
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    let mut h = panel_harness(m);
    h.run();
    h.run();
    assert!(
        spin_values(&h).contains(&"-3.0".to_string()),
        "{:?}",
        spin_values(&h)
    );
    h.get_by_label("clamped to ≥ 0");
    assert!(!h.state().sets_doc.as_ref().unwrap().is_dirty());
    assert_eq!(
        rock_settings(&h)["roughness_scale"],
        serde_json::json!(-3.0)
    );
}

#[test]
fn an_unset_number_without_a_default_says_not_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    m.set_json_value(
        "landscape/rock01",
        "emissive",
        Some(serde_json::json!(true)),
    );
    let mut h = harness(m);
    h.run();
    assert!(spin_values(&h).contains(&"not set".to_string()));
}

fn rock_with(keys: &[(&str, serde_json::Value)]) -> Harness<'static, AppModel> {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    for (k, v) in keys {
        m.set_json_value("landscape/rock01", k, Some(v.clone()));
    }
    let mut h = harness(m);
    h.run();
    h
}

fn disabled(h: &Harness<'static, AppModel>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().is_disabled()
}

#[test]
fn multilayer_and_subsurface_exclude_each_other() {
    let h = rock_with(&[("subsurface", serde_json::json!(true))]);
    assert!(disabled(&h, "Multilayer (coat)"));
    assert!(
        !disabled(&h, "Subsurface"),
        "the one that is on can be turned off"
    );
    let h = rock_with(&[("multilayer", serde_json::json!(true))]);
    assert!(disabled(&h, "Subsurface"));
}

#[test]
fn hair_greys_the_six_features_it_excludes() {
    let h = rock_with(&[("hair", serde_json::json!(true))]);
    for l in [
        "Emissive",
        "Parallax",
        "Subsurface",
        "Multilayer (coat)",
        "Glint",
        "Fuzz",
    ] {
        assert!(disabled(&h, l), "{l} is greyed while hair is on");
    }
    assert!(!disabled(&h, "Hair"));
}

#[test]
fn unticking_a_switch_removes_its_key() {
    let mut h = rock_with(&[("parallax", serde_json::json!(true))]);
    h.get_by_label("Parallax").click();
    h.run();
    assert!(!rock_settings(&h).contains_key("parallax"));
}

#[test]
fn the_tri_buttons_write_true_false_and_remove() {
    let mut h = rock_with(&[]);
    h.state_mut().settings.advanced_open = true;
    h.run();
    // The first Tri row of the Advanced drawer is vertex_colors.
    let nth = |h: &Harness<'static, AppModel>, l: &str| {
        h.query_all_by_label(l).next().map(|n| n.rect()).is_some()
    };
    assert!(nth(&h, "on"));
    h.query_all_by_label("on").next().unwrap().click();
    h.run();
    assert_eq!(rock_settings(&h)["vertex_colors"], serde_json::json!(true));
    h.query_all_by_label("off").next().unwrap().click();
    h.run();
    assert_eq!(rock_settings(&h)["vertex_colors"], serde_json::json!(false));
    h.query_all_by_label("not set").next().unwrap().click();
    h.run();
    assert!(!rock_settings(&h).contains_key("vertex_colors"));
}

#[test]
fn an_empty_json_name_restores_the_default() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let default = sdc_gui::model::pbr_export::json_name(&m);
    m.set_json_name("rock-pack");
    m.set_json_name("  ");
    assert_eq!(sdc_gui::model::pbr_export::json_name(&m), default);
    assert_eq!(m.sets_doc.as_ref().unwrap().json_name(), None);
}

#[test]
fn the_json_name_is_editable_and_undoable() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_name("rock-pack");
    assert_eq!(sdc_gui::model::pbr_export::json_name(&m), "rock-pack");
    m.undo();
    assert_eq!(sdc_gui::model::pbr_export::json_name(&m), "textures");
}

#[test]
fn glint_and_fuzz_switch_the_gui_keys_and_exclude_each_other() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(h.query_by_label("Screen-space scale").is_none());
    h.get_by_label("Glint").click();
    h.run();
    h.get_by_label("Screen-space scale");
    let s = rock_settings(&h);
    assert_eq!(s["glint_on"], serde_json::json!(true));
    assert!(
        !s.contains_key("glint"),
        "the switch never writes the table key"
    );
    h.get_by_label("Fuzz").click();
    h.run();
    assert!(
        !rock_settings(&h).contains_key("fuzz_on"),
        "fuzz is greyed while glint is on"
    );
}

#[test]
fn colours_are_edited_as_rgb_0_to_255() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    m.set_json_value(
        "landscape/rock01",
        "emissive",
        Some(serde_json::json!(true)),
    );
    m.set_json_value(
        "landscape/rock01",
        "emissive_color",
        Some(serde_json::json!([10, 20, 30])),
    );
    let mut h = harness(m);
    h.run();
    h.get_by_label("Emissive colour");
    // Three drag values (R, G, B) hold the 0-255 integers.
    for v in ["10", "20", "30"] {
        assert!(
            h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
                .any(|n| n.value().as_deref() == Some(v)),
            "a drag value shows {v}"
        );
    }
}

/// Tooltips show at once, so a hover needs no waiting.
fn instant_tooltips(h: &Harness<'static, AppModel>) {
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
}

#[test]
fn a_filled_coat_slot_greys_fuzz_with_its_reason() {
    use sdc_gui::model::sets::{set_views, SlotFill, SlotKind};
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([128u8, 128, 255]))
        .save(r.join("rock01_cnr.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let v = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap();
    assert!(
        !matches!(v.slots.get(&SlotKind::Coat), None | Some(SlotFill::Empty)),
        "the fixture puts _cnr in the coat slot: {:?}",
        v.slots
    );
    let mut h = harness(m);
    instant_tooltips(&h);
    h.run();
    let fuzz = h.get_by_label("Fuzz");
    assert!(
        fuzz.accesskit_node().is_disabled(),
        "fuzz is greyed while the coat slot is filled"
    );
    assert!(
        h.query_by_label_contains("the set has a coat normal")
            .is_none(),
        "the reason shows only on hover"
    );
    fuzz.hover();
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("the set has a coat normal (_cnr)")
            .is_some(),
        "the reason shows on hover"
    );
    // The coat normal switch is not greyed by this (only by a filled fuzz slot).
    assert!(!h
        .get_by_label("Multilayer (coat)")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn a_read_only_sets_doc_disables_the_controls_and_explains_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    // A sets document that failed to parse is read-only.
    let bad = d.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("sdc-project.toml"), "this is = = not toml\n").unwrap();
    m.sets_doc = Some(sdc_gui::sets_doc::SetsDoc::open(&bad));
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 500.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, "landscape/rock01"),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    instant_tooltips(&h);
    h.run();
    let n = h.get_by_label("Parallax");
    assert!(n.accesskit_node().is_disabled(), "the control is disabled");
    n.hover();
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("sdc-project.toml").is_some(),
        "the reason shows on hover"
    );
}

#[test]
fn a_filled_fuzz_slot_greys_the_coat_normal_switch() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |_, _| image::Rgb([200u8, 200, 200]))
        .save(r.join("rock01_f.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    m.set_json_value(
        "landscape/rock01",
        "multilayer",
        Some(serde_json::json!(true)),
    );
    let mut h = harness(m);
    h.run();
    let node = h.get_by_label("Coat normal (_cnr)");
    assert!(node.accesskit_node().is_disabled());
    assert!(!h
        .get_by_label("Coat diffuse")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn the_header_has_the_json_name_and_the_drawer_has_the_advanced_keys() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    m.settings.advanced_open = true;
    let mut h = harness(m);
    h.run();
    h.get_by_label("JSON:");
    h.get_by_label_contains("PGPatcher (advanced)");
    h.get_by_label("Match slot 1 (diffuse)");
    assert!(h.query_by_label("Match normal").is_none());
    h.get_by_label("Lock diffuse");
}

/// Final review Minor 2 (parked Task 14): the slot cards' Format and Opaque write `sdc.toml`; while it is read-only
/// they are disabled and say why on hover (the edit would be dropped silently).
#[test]
fn a_read_only_sdc_toml_disables_format_and_opaque_and_explains_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let bad = d.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("sdc.toml"), "this is = = not toml\n").unwrap();
    m.project = Some(sdc_gui::project::ProjectDoc::open(&bad));
    assert!(m.project.as_ref().unwrap().read_only());
    let mut h = harness(m);
    instant_tooltips(&h);
    h.run();
    let combos: Vec<_> = h
        .query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .filter(|n| {
            n.accesskit_node()
                .value()
                .or(n.accesskit_node().label())
                .is_some_and(|t| t.contains("BC"))
        })
        .collect();
    assert!(!combos.is_empty(), "the slot cards show a format");
    assert!(
        combos.iter().all(|n| n.accesskit_node().is_disabled()),
        "every format is disabled"
    );
    let opaque = h.get_by_label("Opaque");
    assert!(opaque.accesskit_node().is_disabled());
    opaque.hover();
    h.run();
    h.run();
    assert!(
        h.query_by_label_contains("sdc.toml is read-only").is_some(),
        "the reason shows on hover"
    );
}

#[test]
fn hovering_a_pills_warning_badge_lists_the_warnings() {
    use engine::codes::{Code, Warning};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    // The fixture's other sets carry SHADER-MISSING-MAP; only rock01's member warnings are under test.
    for s in m.sets.iter_mut() {
        s.warnings.clear();
    }
    let key = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
        .members[0]
        .clone();
    let member = m.items.iter_mut().find(|i| i.key == key).unwrap();
    let name = member.name.clone();
    member.warnings = vec![Warning::new(Code::LossAlphaData, "the alpha would be lost")];
    let total: usize = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
        .members
        .iter()
        .filter_map(|k| m.item(k))
        .map(|i| i.warnings.len())
        .sum();
    let mut h = harness(m);
    instant_tooltips(&h);
    h.run();
    assert!(
        h.query_by_label_contains("the alpha would be lost")
            .is_none(),
        "the warning shows only on hover"
    );
    h.get_by_label(&format!("⚠ {total}")).hover();
    h.run();
    h.run();
    h.get_by_label_contains(&format!("{name}: LOSS-ALPHA-DATA the alpha would be lost"));
}

/// Every item of a PBR project carries exactly `ws` as its warnings.
fn with_only_warnings(ws: &[engine::codes::Warning]) -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
    // Set warnings (SHADER-*) count on the pill too.
    for s in m.sets.iter_mut() {
        s.warnings.clear();
    }
    let key = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
        .members[0]
        .clone();
    m.items.iter_mut().find(|i| i.key == key).unwrap().warnings = ws.to_vec();
    (d, m)
}

#[test]
fn an_info_only_warning_shows_no_badge_on_the_pill_or_the_tile() {
    use engine::codes::{Code, Warning};
    let (_d, m) = with_only_warnings(&[Warning::new(Code::AlphaPbrNormalDropped, "dropped")]);
    let mut h = harness(m);
    h.run();
    assert_eq!(
        h.query_all_by_label_contains("⚠").count(),
        0,
        "no amber badge"
    );
}

#[test]
fn a_warning_level_warning_still_shows_the_pill_and_tile_badge() {
    use engine::codes::{Code, Warning};
    let (_d, m) = with_only_warnings(&[
        Warning::new(Code::LossReencode, "re-encoded"),
        Warning::new(Code::AlphaPbrNormalDropped, "dropped"),
    ]);
    let mut h = harness(m);
    h.run();
    // The pill counts the Warning only, not the Info beside it; the tile shows the bare badge.
    h.get_by_label("⚠ 1");
    h.get_by_label("⚠");
}

/// A set's own warnings (`SetView::set_warnings`, the plan's SHADER-* lines) count in its pill's ⚠ badge beside
/// its members' warnings; an Info set line (SHADER-AUTO) does not.
#[test]
fn a_sets_own_warnings_count_in_the_pill_badge() {
    use engine::codes::{Code, Warning};
    let (_d, mut m) = with_only_warnings(&[Warning::new(Code::LossReencode, "re-encoded")]);
    let key = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
        .key;
    m.sets.iter_mut().find(|s| s.key == key).unwrap().warnings = vec![
        Warning::new(Code::ShaderMissingMap, "the pbr shader needs _rmaos"),
        Warning::new(Code::ShaderAuto, "shader pbr picked automatically (pbr)"),
    ];
    let v = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.key == key)
        .unwrap();
    assert_eq!(
        v.set_warnings.len(),
        2,
        "the view carries the set's warnings"
    );
    let mut h = harness(m);
    instant_tooltips(&h);
    h.run();
    h.get_by_label("⚠ 2").hover();
    h.run();
    h.run();
    h.get_by_label_contains("landscape/rock01: SHADER-MISSING-MAP the pbr shader needs _rmaos");
}

/// M6 Track E spec §3.1 (Note U2): a `_coat` source in the Subsurface slot titles its card "Coat colour + strength".
#[test]
fn a_coat_source_titles_the_subsurface_card_coat_colour_and_strength() {
    use sdc_gui::model::sets::{slot_label, SlotKind};
    assert_eq!(
        slot_label(SlotKind::Subsurface, "landscape/rock01_s.png"),
        "Subsurface _s"
    );
    assert_eq!(
        slot_label(SlotKind::Subsurface, "landscape/rock01_Coat.png"),
        "Coat colour + strength"
    );
    assert_eq!(
        slot_label(SlotKind::Diffuse, "landscape/rock01.png"),
        "Diffuse"
    );
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgba([200u8, 180, 160, (x * 4) as u8]))
        .save(r.join("rock01_coat.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1200.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Coat colour + strength");
    assert!(h.query_by_label("Subsurface _s").is_none());
}

fn text_inputs(h: &Harness<'static, AppModel>) -> Vec<String> {
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .filter_map(|n| n.value())
        .collect()
}

/// F1 spec §3.2: the Matching header lists the default match1 and says how the rows combine.
#[test]
fn the_matching_header_lists_the_default_match1() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel_harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label("Matching");
    h.get_by_label("The entry applies when any of these matches.");
    assert!(
        text_inputs(&h).contains(&r"landscape\rock01".to_string()),
        "{:?}",
        text_inputs(&h)
    );
    assert!(h.query_by_label_contains("matches nothing").is_none());
}

fn match_toggles(h: &Harness<'static, AppModel>) -> Vec<bool> {
    use eframe::egui::accesskit::Toggled;
    h.query_all_by_label("Match on this")
        .map(|n| n.accesskit_node().toggled() == Some(Toggled::True))
        .collect()
}

/// F1 spec §3.2, §6 "Toggles": the diffuse card is on by default and the normal card off. Removing the match1 row
/// turns the diffuse toggle off and warns; the normal toggle adds match2.
#[test]
fn the_card_toggles_mirror_the_matching_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert_eq!(match_toggles(&h), vec![true, false], "diffuse, normal");
    // The Matching header sits below the visual groups: bring it into view before a pointer click.
    h.get_by_label("×").scroll_to_me();
    h.run();
    h.get_by_label("×").click();
    h.run();
    assert_eq!(rock_settings(&h)["match1"], serde_json::json!(""));
    assert_eq!(match_toggles(&h), vec![false, false]);
    h.get_by_label_contains("this entry matches nothing");
    h.query_all_by_label("Match on this")
        .nth(1)
        .unwrap()
        .scroll_to_me();
    h.run();
    h.query_all_by_label("Match on this")
        .nth(1)
        .unwrap()
        .click();
    h.run();
    assert_eq!(
        rock_settings(&h)["match2"],
        serde_json::json!(r"landscape\rock01_n")
    );
    assert!(text_inputs(&h).contains(&r"landscape\rock01_n".to_string()));
    h.query_all_by_label("Match on this")
        .next()
        .unwrap()
        .scroll_to_me();
    h.run();
    h.query_all_by_label("Match on this")
        .next()
        .unwrap()
        .click();
    h.run();
    assert!(
        !rock_settings(&h).contains_key("match1"),
        "back to the derived default"
    );
    assert_eq!(match_toggles(&h), vec![true, true]);
}

/// F1 spec §6 "Toggles": + offers slots 3–8 only when the set has a texture there.
#[test]
fn the_plus_menu_offers_only_filled_slots_3_to_8() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel_harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label("+").click();
    h.run();
    assert!(
        h.query_by_label_contains("3 · ").is_none(),
        "slot 3 is empty"
    );
    h.get_by_label("6 · RMAOS").click();
    h.run();
    assert_eq!(
        rock_settings(&h)["match6"],
        serde_json::json!(r"landscape\rock01_rmaos")
    );
}

/// F1 Review Focus 4 (plan): leaving a row unchanged writes nothing: the default match1 does not become a key, and
/// the project stays clean.
#[test]
fn leaving_a_matching_row_unchanged_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.save().unwrap();
    let mut h = panel_harness(m);
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.value().as_deref() == Some(r"landscape\rock01"))
        .expect("the match1 row")
        .focus();
    h.run();
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert!(!rock_settings(&h).contains_key("match1"));
    assert!(!h.state().sets_doc.as_ref().unwrap().is_dirty());
}
