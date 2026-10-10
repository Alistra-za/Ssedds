mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::RawMap;
use sdc_gui::model::inspector::*;
use sdc_gui::model::preview::{EncodedState, SourceState};
use sdc_gui::model::AppModel;
use sdc_gui::settings::Bc1Choice;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

#[test]
fn the_inspector_shows_provenance_and_differences_from_the_base() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    let reg = m.session.as_ref().unwrap().engine().registry().clone();
    let rows = inspector_rows(&job, &reg);
    let t = rows.iter().find(|r| r.field == "alpha.threshold").unwrap();
    assert_eq!(t.provenance, "project_file");
    assert!(t.differs_from_base);
    assert!(rows.iter().any(|r| !r.differs_from_base));
}

#[test]
fn fill_values_are_written_to_defaults() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_fill(RawMap::Metallic, Some(255));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m
        .item("landscape/rock01_rmaos")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_eq!(job.provenance["pack.g"].to_string(), "fill:255");
    assert_eq!(
        m.project.as_ref().unwrap().fill(RawMap::Metallic),
        Some(255)
    );
}

#[test]
fn unmatched_entries_are_listed_and_removable() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(
        "landscape/rock01_rmaos",
        "alpha.threshold",
        serde_json::json!(90),
    );
    m.set_target(engine::config::Target::Cpm);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.unmatched_keys, vec!["landscape/rock01_rmaos".to_string()]);
    m.remove_unmatched("landscape/rock01_rmaos");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.unmatched_keys.is_empty());
    assert!(!m
        .project
        .as_ref()
        .unwrap()
        .file_keys()
        .contains(&"landscape/rock01_rmaos".to_string()));
}

#[test]
fn preview_size_and_bc1_choice_follow_their_rules() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_preview_max(16384);
    assert_eq!(
        m.settings.preview_max, 4096,
        "16384 needs the explicit setting"
    );
    m.settings.allow_16k_preview = true;
    m.set_preview_max(16384);
    assert_eq!(m.settings.preview_max, 16384);
    m.set_bc1_choice(Bc1Choice::Nvidia);
    assert!(m.restart_needed);
    m.set_bc1_choice(Bc1Choice::Ideal);
    assert!(!m.restart_needed);
}

#[test]
fn turning_16384_off_lowers_a_16384_limit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_allow_16k_preview(true);
    m.set_preview_max(16384);
    m.set_allow_16k_preview(false);
    assert!(!m.settings.allow_16k_preview);
    assert!(m.settings.preview_max <= 8192);
    m.set_preview_max(16384);
    assert!(m.settings.preview_max <= 8192, "still refused");
}

/// Task 15's carry-over: a limit change restarts the expanded preview.
#[test]
fn changing_the_preview_limit_restarts_the_expanded_preview() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(before) = m.preview.source.clone() else {
        panic!("no source")
    };
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));

    m.set_preview_max(2048);
    assert!(
        matches!(m.preview.encoded, EncodedState::Stale(_)),
        "the result was made under the old limit"
    );
    let t0 = Instant::now();
    m.tick(t0);
    m.tick(t0 + sdc_gui::model::preview::DEBOUNCE);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(after) = &m.preview.source else {
        panic!("no source")
    };
    assert!(!Arc::ptr_eq(&before, after), "the source was read again");

    // Live encode re-encodes after a further change, including the 16k toggle.
    m.set_live(true);
    m.set_allow_16k_preview(true);
    m.set_preview_max(16384);
    let t1 = Instant::now();
    m.tick(t1);
    m.tick(t1 + sdc_gui::model::preview::DEBOUNCE);
    assert!(matches!(
        m.preview.encoded,
        EncodedState::Encoding { .. } | EncodedState::Done(_)
    ));
}

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
fn the_open_drawer_shows_its_sections() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.advanced_open = true;
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    for label in [
        "Preset inspector",
        "Batch",
        "Packing fill values",
        "Preview",
        "Advanced texture fields",
        "Unmatched [files] entries",
        "Show only differences",
        "BC1 decoder",
    ] {
        assert!(
            h.query_all_by_label_contains(label).count() >= 1,
            "no {label:?} in the drawer"
        );
    }
    h.get_by_label_contains("cs-quality");
}

#[test]
fn the_strip_opens_the_drawer_and_close_collapses_it() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    assert!(!h.state().settings.advanced_open);
    assert!(h.query_by_label("Preset inspector").is_none());
    h.get_by_label("Advanced").click();
    h.run();
    assert!(h.state().settings.advanced_open);
    h.get_by_label("Preset inspector");
    h.get_by_label("Close").click();
    h.run();
    assert!(!h.state().settings.advanced_open);
}

#[test]
fn ticking_fill_writes_a_fill_and_remove_drops_an_unmatched_entry() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_field(
        "landscape/rock01_rmaos",
        "alpha.threshold",
        serde_json::json!(90),
    );
    model.set_target(engine::config::Target::Cpm);
    assert!(model.wait_idle(Duration::from_secs(60)));
    model.settings.advanced_open = true;
    let mut h = harness(model);
    h.run();
    h.get_by_label("landscape/rock01_rmaos");
    h.get_by_label("Remove").click();
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
    assert!(h.state().unmatched_keys.is_empty());

    let first_fill = h.query_all_by_label("Fill").next().unwrap();
    first_fill.click();
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
    let fill = h.state().project.as_ref().unwrap().fill(RawMap::Roughness);
    assert_eq!(fill, Some(128));
}

#[test]
fn the_restart_notice_appears_when_the_bc1_choice_differs() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.advanced_open = true;
    let mut h = harness(model);
    h.run();
    assert!(h
        .query_by_label_contains("restart sdc-gui to apply")
        .is_none());
    h.state_mut().set_bc1_choice(Bc1Choice::Amd);
    h.run();
    h.get_by_label_contains("restart sdc-gui to apply");
}

#[test]
fn the_advanced_texture_fields_carry_their_locks_and_edit_through_the_model() {
    use sdc_gui::model::fields::advanced_field_views;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(
        "landscape/rock01.png",
        "encoder.bc45_search_radius",
        serde_json::json!("max"),
    );
    m.set_field("landscape/rock01.png", "header", serde_json::json!("dx10"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/rock01.png").unwrap().job.clone().unwrap();
    let views = advanced_field_views(m.target, &job);
    let names: Vec<_> = views.iter().map(|v| v.field).collect();
    assert_eq!(
        names,
        [
            "encoder.perceptual",
            "encoder.bc1_transparent_black",
            "encoder.bc45_search_radius",
            "encoder.rgbcx_level",
            "header"
        ]
    );
    let header = views.iter().find(|v| v.field == "header").unwrap();
    assert_eq!(header.value, serde_json::json!("dx10"));
    assert!(header.edited);
    let radius = views
        .iter()
        .find(|v| v.field == "encoder.bc45_search_radius")
        .unwrap();
    assert!(radius.lock.is_some(), "a colour map is not BC4/BC5");
}
