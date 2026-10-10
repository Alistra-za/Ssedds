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
