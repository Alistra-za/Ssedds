//! Note FB8 (amendment D): no code text in the UI. Problems rows and banners carry a sentence that names the action;
//! the code stays on hovers and in the Messages log.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use sdc_gui::model::AppModel;
use std::time::Duration;
use support::*;

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h
}

/// Every item of a PBR project carries exactly `ws` as its warnings (bottom_panel.rs::with_only_warnings).
fn with_only_warnings(ws: &[Warning]) -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
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
    m.invalidate_views();
    (d, m)
}

const ALPHA_SENTENCE: &str =
    "This alpha channel carries data the format would drop — choose what it means";

#[test]
fn no_code_text_in_problems_rows() {
    let (_d, mut m) = with_only_warnings(&[
        Warning::new(Code::LossAlphaData, "alpha is lost in this format"),
        Warning::new(Code::ChkRange, "albedo values leave the 30-240 range"),
    ]);
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.run();
    h.get_by_label(ALPHA_SENTENCE);
    for prefix in ["LOSS-", "CHK-"] {
        assert_eq!(
            h.query_all_by_label_contains(prefix).count(),
            0,
            "a visible label shows {prefix}"
        );
    }
    // The hover keeps the engine's message and the code.
    h.get_by_label(ALPHA_SENTENCE).hover();
    h.run();
    h.run();
    h.get_by_label_contains("LOSS-ALPHA-DATA");
    h.get_by_label_contains("alpha is lost in this format");
}

#[test]
fn a_failed_row_and_a_decide_row_say_it_in_words() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.items[0].status = sdc_gui::model::items::ItemStatus::Error(vec![Code::DimNpot]);
    m.invalidate_views();
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.run();
    assert_eq!(h.query_all_by_label_contains("DIM-NPOT").count(), 0);
    assert!(h
        .query_all_by_label_contains("Needs a decision: ")
        .all(|n| !n
            .accesskit_node()
            .label()
            .unwrap_or_default()
            .contains("DIM-")));
}

#[test]
fn no_code_text_in_the_banner() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let raw = "CFG-INVALID: preset 'mine': suffix alias 'rmaos' is a canonical suffix";
    m.project.as_mut().unwrap().set_error(raw);
    m.refresh_banner();
    let banner = m.banner.clone().expect("a banner");
    assert!(!banner.contains("CFG-"), "{banner}");
    assert!(banner.contains("suffix alias 'rmaos' is a canonical suffix"));
    // The Messages log keeps the line as the engine wrote it, once however often the banner is rebuilt.
    m.refresh_banner();
    let logged = m
        .messages
        .entries()
        .filter(|e| e.text.contains("CFG-INVALID"))
        .count();
    assert_eq!(logged, 1);
}

/// C12: each Decide… entry says what it does.
#[test]
fn every_decide_entry_has_a_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Decide…").click();
    h.run();
    use engine::config::AlphaPolicy::*;
    for (entry, policy) in [
        ("Remove", Remove),
        ("Cutout", Cutout),
        ("Blend", Blend),
        ("Cutout + Blend", CutoutBlend),
        ("Data", Data),
    ] {
        let hint = sdc_gui::ui::settings_panel::alpha_hint(policy);
        assert!(!hint.is_empty(), "{entry}");
        h.get_by_label(entry).hover();
        h.run();
        h.run();
        h.get_by_label(hint);
    }
    // Data's hover says what the alpha becomes.
    assert!(sdc_gui::ui::settings_panel::alpha_hint(Data).contains("kept as data"));
}
