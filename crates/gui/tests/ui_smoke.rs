mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, Selection};
use sdc_gui::settings::GuiSettings;
use support::*;

/// A harness over `model`. The crate bundles no fonts, so the harness needs the system fonts, as the app does.
fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_empty_window_shows_its_regions() {
    let mut h = harness(AppModel::placeholder(GuiSettings::default()));
    h.run();
    for label in [
        "Open folder…",
        "Mode",
        "Export",
        "Save",
        "Textures",
        "Sets",
        "Advanced",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("Open a folder");
}

#[test]
fn an_opened_folder_shows_items_filters_and_the_undecided_count() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    for label in [
        "leaf.png",
        "rock01_normal.png",
        "Needs decision",
        "Not in a set",
    ] {
        h.get_by_label_contains(label);
    }
    // The packed output's tile, and the two raw maps that say they are packed into it.
    assert_eq!(h.query_all_by_label_contains("rock01_rmaos").count(), 3);
    h.get_by_label_contains("need a decision");
    // A set is named by its path under the export folder.
    h.get_by_label("landscape/rock01");
}

#[test]
fn collapsing_the_columns_hides_the_thumbnails() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.columns_collapsed = true;
    let mut h = harness(model);
    h.run();
    assert!(h.query_by_label_contains("leaf.png").is_none());
}

#[test]
fn clicking_a_tile_selects_it_and_the_needs_decision_filter_narrows_the_grid() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label_contains("rock01_normal.png").click();
    h.run();
    assert!(
        matches!(&h.state().selection, Selection::Item(k) if k.ends_with("rock01_normal.png")),
        "selection was {:?}",
        h.state().selection
    );
    h.get_by_label("Needs decision").click();
    h.run();
    assert!(h.state().filter.needs_decision);
    h.get_by_label_contains("leaf.png");
    assert!(h.query_by_label_contains("rock01_roughness.png").is_none());
}

#[test]
fn closing_with_unsaved_edits_asks_and_discard_drops_them() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_target(engine::config::Target::Vanilla);
    assert!(model.is_dirty());
    assert!(!model.request_close());
    let mut h = harness(model);
    h.run();
    h.get_by_label("Cancel");
    // egui hit-tests against the previous frame: let the pointer settle on the button before the click.
    h.get_by_label("Discard").hover();
    h.run();
    h.get_by_label("Discard").click();
    h.run();
    assert!(!h.state().is_dirty());
    assert!(h.state().dialog.is_none());
}

#[test]
fn the_columns_keep_their_width_through_a_collapse_and_expand() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    // The Sets column starts where the Textures column ends.
    let before = h.get_by_label("Sets").rect().min.x;
    assert!(before > 300.0, "Sets starts at {before}");
    h.state_mut().settings.columns_collapsed = true;
    h.run();
    assert!(h
        .query_by_label("Sets")
        .is_some_and(|n| n.rect().width() < 30.0));
    h.state_mut().settings.columns_collapsed = false;
    h.run();
    let after = h.get_by_label("Sets").rect().min.x;
    assert!(after > 300.0, "Sets starts at {after} after expanding");
}

#[test]
fn selecting_a_texture_shows_its_decisions_and_groups() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_normal.png");
    let mut h = harness(model);
    h.run();
    for label in [
        "Needs a decision",
        "OpenGL",
        "DirectX",
        "Colour",
        "Content",
        "DDS tag",
        "Compression",
    ] {
        h.get_by_label_contains(label);
    }
}

#[test]
fn selecting_a_set_shows_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    model.select_set(&key);
    let mut h = harness(model);
    h.run();
    // The recipe line names the packed output and its status; the Sets column's pill says only "rmaos: run".
    h.get_by_label_contains("rock01_rmaos.dds: run");
}

fn settle(h: &mut Harness<'static, AppModel>) {
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    h.run();
}

/// Click `label` the way a pointer does: settle on it first, because egui hit-tests against the previous frame.
fn press(h: &mut Harness<'static, AppModel>, label: &str) {
    h.get_by_label(label).hover();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

#[test]
fn a_green_convention_button_writes_an_explicit_value() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_normal.png");
    let mut h = harness(model);
    h.run();
    press(&mut h, "OpenGL");
    settle(&mut h);
    let item = h.state().item("landscape/rock01_normal.png").unwrap();
    let job = item.job.as_ref().unwrap();
    assert_eq!(
        job.profile.channels.green_convention,
        Some(engine::config::GreenConvention::Opengl)
    );
    assert!(h.state().is_dirty());
    // The green question is answered; the sub-type question is still open, so the box stays.
    assert!(h.query_by_label("OpenGL").is_none());
    h.get_by_label_contains("Needs a decision");
}

#[test]
fn an_alpha_button_writes_the_policy_and_the_reset_button_takes_it_back() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    // A plain diffuse takes its alpha meaning from the mesh, so once the sub-type is decided the alpha is asked.
    model.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    h.get_by_label_contains("Needs a decision");
    // The decision box offers the five meanings as buttons.
    for label in ["Remove", "Blend", "Cutout + Blend", "Data"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label}");
    }
    let cutout = h
        .query_all_by_label("Cutout")
        .next()
        .expect("a Cutout button");
    cutout.hover();
    h.run();
    h.query_all_by_label("Cutout").next().unwrap().click();
    h.run();
    settle(&mut h);
    let job = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_eq!(
        job.profile.alpha.policy,
        engine::config::AlphaPolicy::Cutout
    );
    assert_eq!(job.provenance["alpha.policy"].to_string(), "project_file");
    assert!(h.query_by_label_contains("Needs a decision").is_none());
    // The edited field offers a reset.
    // Two fields are edited now (sub-type, then alpha): the second reset is the alpha one.
    assert_eq!(h.query_all_by_label("↺").count(), 2);
    h.query_all_by_label("↺").nth(1).unwrap().hover();
    h.run();
    h.query_all_by_label("↺").nth(1).unwrap().click();
    h.run();
    settle(&mut h);
    let item = h.state().item("landscape/leaf.png").unwrap();
    assert_ne!(
        item.job.as_ref().unwrap().provenance["alpha.policy"].to_string(),
        "project_file"
    );
    assert!(!item.decisions.is_empty(), "the decision is open again");
}

#[test]
fn the_threshold_commits_when_the_drag_stops_not_while_dragging() {
    use eframe::egui::{Event, Modifiers, PointerButton};
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Alpha(engine::config::AlphaPolicy::Cutout),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    let before = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .profile
        .alpha
        .threshold;
    let rect = h
        .query_all_by_role(eframe::egui::accesskit::Role::Slider)
        .next()
        .expect("the threshold slider")
        .rect();
    let at = |f: f32| {
        eframe::egui::pos2(
            rect.left() + 10.0 + (rect.width() - 20.0) * f,
            rect.center().y,
        )
    };
    let ev = |h: &mut Harness<'static, AppModel>, e: Event| {
        h.input_mut().events.push(e);
        h.step();
    };
    ev(&mut h, Event::PointerMoved(at(0.2)));
    ev(
        &mut h,
        Event::PointerButton {
            pos: at(0.2),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    ev(&mut h, Event::PointerMoved(at(0.5)));
    ev(&mut h, Event::PointerMoved(at(0.8)));
    // Still dragging: nothing was sent to the model, so no re-plan is running.
    assert!(!h.state().busy(), "a re-plan started mid-drag");
    ev(
        &mut h,
        Event::PointerButton {
            pos: at(0.8),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    h.run();
    settle(&mut h);
    let job = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_ne!(job.profile.alpha.threshold, before);
    assert_eq!(
        job.provenance["alpha.threshold"].to_string(),
        "project_file"
    );
}

#[test]
fn a_packed_item_lists_its_channels_and_a_raw_map_points_to_the_output() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let packed = model
        .items
        .iter()
        .find(|i| i.kind == sdc_gui::model::items::ItemKind::Packed)
        .unwrap()
        .key
        .clone();
    model.select_item(&packed);
    let mut h = harness(model);
    h.run();
    h.get_by_label("Channels");
    h.get_by_label_contains("R: from ");
    h.get_by_label_contains("B: from rock01_ao.png");
    h.get_by_label("A: default value 1");
    // A raw map says it is packed and offers the way to its output.
    h.state_mut().select_item("landscape/rock01_roughness.png");
    h.run();
    press(&mut h, "Show packed output");
    assert_eq!(h.state().selection, Selection::Item(packed));
}

#[test]
fn a_locked_field_is_disabled_and_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_target(engine::config::Target::Vanilla);
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/rock01.png");
    let mut h = harness(model);
    h.run();
    h.get_by_label("_SRGB is only written under cs");
}
