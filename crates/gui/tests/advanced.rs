mod support;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable};
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
    assert_eq!(m.batch_preset, "cs-quality");
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
fn a_project_wide_fill_still_applies_as_a_fallback() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.project
        .as_mut()
        .unwrap()
        .set_fill(RawMap::Metallic, Some(255));
    m.replan();
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
    m.set_target(engine::config::Target::Vanilla);
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
}

/// Turning 16384 off while it is the limit is the toggle path that changes the effective limit; a change that
/// leaves the limit as it was leaves a finished result alone.
#[test]
fn the_16k_toggle_restarts_only_when_the_effective_limit_changes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_allow_16k_preview(true);
    m.set_preview_max(16384);
    m.expand("landscape/rock01_rmaos");
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(before) = m.preview.source.clone() else {
        panic!("no source")
    };
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));

    // No-ops: the same size again, and the box ticked again.
    m.set_preview_max(16384);
    m.set_allow_16k_preview(true);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
    assert!(!m.preview.scheduled(), "nothing was scheduled");

    m.set_allow_16k_preview(false);
    assert_eq!(m.settings.preview_max, 8192);
    assert!(
        matches!(m.preview.encoded, EncodedState::Stale(_)),
        "the result was made under the 16384 limit"
    );
    let t0 = Instant::now();
    m.tick(t0);
    m.tick(t0 + sdc_gui::model::preview::DEBOUNCE);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(after) = &m.preview.source else {
        panic!("no source")
    };
    assert!(!Arc::ptr_eq(&before, after), "the source was read again");
}

/// A DDS input has no source side: its stored mips are read again under the new limit.
#[test]
fn changing_the_preview_limit_reopens_an_expanded_dds() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    export_through(&mut m);
    assert!(m.wait_idle(Duration::from_secs(120)));
    let dds = walk(&d.path().join("export"))
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "dds"))
        .expect("the export wrote a .dds");
    std::fs::copy(&dds, d.path().join("textures/landscape/stored.dds")).unwrap();
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.expand("landscape/stored.dds");
    assert!(m.wait_idle(Duration::from_secs(60)));
    let EncodedState::Done(before) = m.preview.encoded.clone() else {
        panic!("not done: {:?}", m.preview.encoded)
    };

    m.set_preview_max(4096);
    assert!(
        matches!(&m.preview.encoded, EncodedState::Done(o) if Arc::ptr_eq(o, &before)),
        "an unchanged limit leaves the result alone"
    );
    m.set_preview_max(2048);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let EncodedState::Done(after) = &m.preview.encoded else {
        panic!("not done: {:?}", m.preview.encoded)
    };
    assert!(!Arc::ptr_eq(&before, after), "the DDS was read again");
    assert!(!after.result.levels.is_empty());
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
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
        "Preview",
        "Advanced texture fields",
        "Unmatched [files] entries",
        "Show only what differs from sdc-base",
        "BC1 decoder",
    ] {
        assert!(
            h.query_all_by_label_contains(label).count() >= 1,
            "no {label:?} in the drawer"
        );
    }
    h.get_by_label_contains("cs-quality");
}

fn drawer_harness(width: f32, model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(width, 1000.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn leaf_model(dir: &std::path::Path) -> AppModel {
    let mut model = opened(dir);
    model.settings.advanced_open = true;
    model.select_item("landscape/leaf.png");
    model
}

/// D10: the first column reads in words; the key rides in the AccessKit name and on hover.
#[test]
fn the_inspector_shows_labels_with_the_key_on_hover() {
    let d = tempfile::tempdir().unwrap();
    let model = leaf_model(d.path());
    let job = model
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    let reg = model.session.as_ref().unwrap().engine().registry().clone();
    let rows = inspector_rows(&job, &reg);
    let labelled: Vec<_> = rows
        .iter()
        .filter(|r| sdc_gui::model::bulk::field_label(&r.field) != r.field)
        .collect();
    assert!(!labelled.is_empty(), "the inspector lists a labelled field");
    let mut h = drawer_harness(1400.0, model);
    h.run();
    for r in &labelled {
        let label = sdc_gui::model::bulk::field_label(&r.field);
        h.get_by_label(&format!("{label} ({})", r.field));
        assert!(
            h.query_by_label(&r.field).is_none(),
            "the raw key {} is shown as text",
            r.field
        );
    }
    // A dotted key with no label shows the readable form.
    for r in rows
        .iter()
        .filter(|r| r.field.contains('.') && sdc_gui::model::bulk::field_label(&r.field) == r.field)
    {
        assert!(
            h.query_by_label(&r.field).is_none(),
            "{} shown raw",
            r.field
        );
    }
}

/// D10, rule 10.3: no inspector label ends past the drawer's edge.
#[test]
fn no_inspector_value_passes_the_drawer_edge() {
    use eframe::egui::containers::panel::PanelState;
    for width in [1100.0, 1280.0, 1600.0] {
        let d = tempfile::tempdir().unwrap();
        let model = leaf_model(d.path());
        let mut h = drawer_harness(width, model);
        h.run();
        let drawer = PanelState::load(&h.ctx, eframe::egui::Id::new("advanced"))
            .unwrap()
            .outer_rect;
        let inspector_top = h.get_by_label("Preset inspector").rect().min.y;
        let batch_top = h.get_by_label("Batch").rect().min.y;
        let mut checked = 0;
        for n in h.query_all_by_role(Role::Label) {
            let r = n.rect();
            if r.min.x >= drawer.min.x && r.min.y > inspector_top && r.min.y < batch_top {
                checked += 1;
                assert!(
                    r.max.x <= drawer.max.x + 0.5,
                    "{width}: {:?} ends at {} past the drawer edge {}",
                    n.accesskit_node().label(),
                    r.max.x,
                    drawer.max.x
                );
            }
        }
        assert!(checked > 10, "{width}: only {checked} labels checked");
    }
}

#[test]
fn the_inspector_names_the_texture() {
    let d = tempfile::tempdir().unwrap();
    let mut h = drawer_harness(1400.0, leaf_model(d.path()));
    h.run();
    h.get_by_label("How leaf.png's settings resolve");
    h.get_by_label("Batch preset: cs-quality");
    h.get_by_role_and_label(Role::Button, "Presets…");
    assert!(h.query_by_label_contains("%APPDATA%").is_none());
    h.get_by_role_and_label(Role::Button, "Presets…").click();
    h.run();
    assert!(h.state().presets_open, "Presets… opens the Presets tab");
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
fn remove_drops_an_unmatched_entry() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_field(
        "landscape/rock01_rmaos",
        "alpha.threshold",
        serde_json::json!(90),
    );
    model.set_target(engine::config::Target::Vanilla);
    assert!(model.wait_idle(Duration::from_secs(60)));
    model.settings.advanced_open = true;
    let mut h = harness(model);
    h.run();
    h.get_by_label("landscape/rock01_rmaos");
    // Note CN: the "Also export recipe" section above pushes the entry below the drawer's fold.
    h.get_by_label("Remove").scroll_to_me();
    h.run();
    h.get_by_label("Remove").click();
    h.run();
    h.state_mut().wait_idle(Duration::from_secs(60));
    h.run();
    assert!(h.state().unmatched_keys.is_empty());

    assert!(
        h.query_by_label("Packing fill values").is_none(),
        "the global fill panel is retired"
    );
}

#[test]
fn the_restart_notice_appears_when_the_bc1_choice_differs() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.advanced_open = true;
    let mut h = harness(model);
    h.run();
    assert!(h
        .query_by_label_contains("restart Tessalith to apply")
        .is_none());
    h.state_mut().set_bc1_choice(Bc1Choice::Amd);
    h.run();
    h.get_by_label_contains("restart Tessalith to apply");
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
            "encoder.bc7_weights",
            "encoder.bc1_transparent_black",
            "encoder.bc45_search_radius",
            "encoder.rgbcx_level",
            "header",
            "resize",
            "drop_top_mip"
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

/// M6 Track D spec §3: the GUI loads the user presets folder and the inspector lists what it loaded.
#[test]
fn the_inspector_lists_loaded_user_presets() {
    let d = tempfile::tempdir().unwrap();
    let presets = d.path().join("user-presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("mine.toml"),
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    let mut m = opened(d.path());
    assert!(m.loaded_user_presets.is_empty());
    m.user_presets = Some(presets.clone());
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.loaded_user_presets.len(), 1);
    assert_eq!(m.loaded_user_presets[0].0, "mine");
    assert!(m.loaded_user_presets[0].1.ends_with("mine.toml"));
    m.settings.advanced_open = true;
    m.select_item("landscape/leaf.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("mine.toml");
}

/// Final review Important 1: a user preset file that fails to load is shown in the inspector with its reason
/// (CFG-PRESET-SKIPPED), under the presets that did load.
#[test]
fn the_inspector_lists_skipped_user_presets_with_their_reason() {
    let d = tempfile::tempdir().unwrap();
    let presets = d.path().join("user-presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("mine.toml"),
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    std::fs::write(
        presets.join("bad.toml"),
        "schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\nbase = \"nope\"\n",
    )
    .unwrap();
    let mut m = opened(d.path());
    m.user_presets = Some(presets.clone());
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let ids: Vec<&str> = m
        .loaded_user_presets
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(ids, ["mine"]);
    assert_eq!(m.skipped_user_presets.len(), 1);
    assert!(m.skipped_user_presets[0].0.ends_with("bad.toml"));
    assert!(m.skipped_user_presets[0]
        .1
        .contains("unknown preset 'nope'"));
    m.settings.advanced_open = true;
    m.select_item("landscape/leaf.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("mine.toml");
    h.get_by_label_contains("bad.toml");
    h.get_by_label_contains("unknown preset 'nope'");
}

#[test]
fn the_reduce_motion_box_saves_and_reaches_the_ui() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.advanced_open = true;
    let mut h = harness(model);
    h.run();
    assert!(!sdc_gui::ui::motion::reduce_motion(&h.ctx));
    h.get_by_label("Reduce motion").click();
    h.run();
    assert!(h.state().settings.reduce_motion);
    assert!(sdc_gui::ui::motion::reduce_motion(&h.ctx));
}
