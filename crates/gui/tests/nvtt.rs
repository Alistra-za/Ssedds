//! M5 Task 8 (DECISIONS Note NV): the NVTT path and status in Advanced, and the texture's Encoder row.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::config::*;
use engine::nvtt::NvttProbe;
use engine::presets::PresetRegistry;
use sdc_gui::model::fields::*;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";

/// The engine's test-only fake nvcompress, which `cargo test --workspace` builds next to the test binaries' folder.
fn fake() -> PathBuf {
    let p = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(|d| d.parent())
        .unwrap()
        .join("nvcompress.exe");
    assert!(
        p.is_file(),
        "{} missing: run `cargo test --workspace`",
        p.display()
    );
    p
}

/// `opened`, with NVTT pointed at the fake and probed, as `main` does at start.
fn with_fake(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    m.settings.nvtt_path = Some(fake());
    m.refresh_nvtt();
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn file_value(m: &AppModel, field: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(ROCK, field)
}

#[test]
fn the_encoder_row_locks_outside_bc7_and_bc6h_and_without_nvtt() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Cs, Subtype::Diffuse);
    p.format = Format::Bc7;
    assert_eq!(field_lock(Target::Cs, &p, "encoder.backend"), None);
    p.format = Format::Bc6hUf16;
    assert_eq!(field_lock(Target::Cs, &p, "encoder.backend"), None);
    for f in [Format::Bc1, Format::Bc3, Format::Bc4, Format::Bc5] {
        p.format = f;
        assert_eq!(
            field_lock(Target::Cs, &p, "encoder.backend"),
            Some("NVTT encodes BC7 and BC6H only"),
            "{f:?}"
        );
        assert!(!field_hidden(&p, "encoder.backend"));
    }
    // Hidden where no encoder runs, like the effort's reason.
    p.format = Format::Rgba8;
    assert!(field_hidden(&p, "encoder.backend"));
    // Not found: locked, except for a texture already on NVTT, which must be able to go back.
    p.format = Format::Bc7;
    p.encoder.backend = Backend::Bc7e;
    assert_eq!(backend_lock(&p, false), Some(NVTT_NOT_FOUND));
    assert_eq!(backend_lock(&p, true), None);
    p.encoder.backend = Backend::Nvtt;
    assert_eq!(backend_lock(&p, false), None);
}

#[test]
fn picking_nvtt_is_one_undo_step_and_built_in_is_the_format_default() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_fake(d.path());
    let job = m.item(ROCK).unwrap().job.clone().unwrap();
    assert_eq!(job.profile.format, Format::Bc7);
    m.set_field(ROCK, "encoder.backend", json!(Backend::Nvtt));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(file_value(&m, "encoder.backend"), Some(json!("nvtt")));
    let job = m.item(ROCK).unwrap().job.clone().unwrap();
    assert_eq!(job.profile.encoder.backend, Backend::Nvtt);
    assert!(
        !job.warnings
            .iter()
            .any(|w| w.code == engine::codes::Code::EncNvttMissing),
        "{:?}",
        job.warnings
    );
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(file_value(&m, "encoder.backend"), None);
    // "Built-in" is written as the texture's own format default, whatever value the row (or a bulk edit) carried.
    m.set_field(ROCK, "encoder.backend", json!(Backend::IspcBc6h));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(file_value(&m, "encoder.backend"), Some(json!("bc7e")));
}

#[test]
fn leaving_bc7_drops_nvtt() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_fake(d.path());
    m.set_field(ROCK, "encoder.backend", json!(Backend::Nvtt));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_format(ROCK, Format::Bc1, ColorTag::Srgb);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(file_value(&m, "encoder.backend"), None);
    let job = m.item(ROCK).unwrap().job.clone().unwrap();
    assert_eq!(job.profile.encoder.backend, Backend::Rgbcx);
}

#[test]
fn the_status_reads_the_probe() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_fake(d.path());
    assert!(m.nvtt_found());
    assert!(
        m.nvtt_status().starts_with("NVTT 3.2.5 at") && m.nvtt_status().ends_with("(tested)"),
        "{}",
        m.nvtt_status()
    );
    m.settings.nvtt_path = Some(d.path().join("no/nvcompress.exe"));
    m.refresh_nvtt();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.nvtt_found());
    assert!(
        m.nvtt_status().starts_with("NVTT not found ("),
        "{}",
        m.nvtt_status()
    );
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
fn the_panel_shows_the_encoder_row_and_the_drawer_the_status() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_fake(d.path());
    m.settings.advanced_open = true;
    m.select_item(ROCK);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Encoder");
    h.get_by_label("NVTT path");
    h.get_by_label_contains("NVTT 3.2.5 at");
    assert!(h.query_by_label(NVTT_NOT_FOUND).is_none());
    h.state_mut().nvtt = Some(NvttProbe::Missing("not on PATH".into()));
    h.run();
    h.get_by_label(NVTT_NOT_FOUND);
    h.get_by_label_contains("NVTT not found (not on PATH)");
}

/// A re-probe that finds something else (on window focus) re-plans: a texture on NVTT then reports it missing.
#[test]
fn a_changed_probe_replans() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_fake(d.path());
    m.set_field(ROCK, "encoder.backend", json!(Backend::Nvtt));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let missing = |m: &AppModel| {
        m.item(ROCK)
            .unwrap()
            .job
            .as_ref()
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.code == engine::codes::Code::EncNvttMissing)
    };
    assert!(!missing(&m));
    m.settings.nvtt_path = Some(d.path().join("no/nvcompress.exe"));
    m.refresh_nvtt();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(missing(&m));
}

/// Audit 2026-10-13b B-I1 (DECISIONS AF3): the probe runs on a worker; its result lands on a later poll, so a slow
/// or wrong program never blocks the UI thread.
#[test]
fn the_probe_runs_off_the_ui_thread() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.nvtt_path = Some(fake());
    m.refresh_nvtt();
    assert_eq!(m.nvtt, None, "nothing yet: the probe is on a worker");
    assert!(m.busy(), "busy until it lands, so the UI keeps polling");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.nvtt_found(), "{}", m.nvtt_status());
}
