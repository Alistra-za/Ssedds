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
    use sdc_gui::model::code_text::code_line;
    use sdc_gui::model::items::{Decision, ItemStatus};
    m.items[0].status = ItemStatus::Error(vec![Code::DimNpot]);
    m.items[1].status = ItemStatus::NeedsDecision;
    m.items[1].decisions = vec![Decision::Acknowledge(Code::DimNpot)];
    m.invalidate_views();
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    h.run();
    let words = code_line(Code::DimNpot);
    let failed = format!("Failed: {words}");
    let decide = format!("Undecided: {words}");
    h.get_by_label(&failed);
    assert!(h.query_all_by_label(&decide).count() >= 1, "{decide}");
    assert_eq!(h.query_all_by_label_contains("DIM-NPOT").count(), 0);
    // The hover of each keeps the code.
    for row in [&failed, &decide] {
        h.query_all_by_label(row).next().unwrap().hover();
        h.run();
        h.run();
        h.get_by_label_contains("DIM-NPOT");
    }
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
    // The code becomes its sentence, then the engine's detail.
    assert!(
        banner.contains(sdc_gui::model::code_text::code_sentence(Code::CfgInvalid)),
        "{banner}"
    );
    assert!(banner.contains("suffix alias 'rmaos' is a canonical suffix"));
    // The Messages log keeps the line as the engine wrote it, once however often the banner is rebuilt.
    m.refresh_banner();
    let logged: Vec<_> = m
        .messages
        .entries()
        .filter(|e| e.text.contains("CFG-INVALID"))
        .collect();
    assert_eq!(logged.len(), 1);
    // The severity follows the code's (CFG-INVALID is an error).
    assert_eq!(
        logged[0].severity,
        sdc_gui::model::messages::Severity::Error
    );
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

/// The panels beside Problems say it in words too: a texture's warning list and a set's layer notes.
#[test]
fn the_settings_panel_and_the_set_editor_say_it_in_words() {
    use sdc_gui::model::code_text::code_sentence;
    let (_d, mut m) =
        with_only_warnings(&[Warning::new(Code::LossReencode, "re-encoded from a DDS")]);
    let set = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap();
    let item = set.members[0].clone();
    m.select_item(&item);
    let mut h = harness(m);
    h.run();
    h.run();
    let words = code_sentence(Code::LossReencode);
    assert!(h.query_all_by_label(words).count() >= 1, "{words}");
    assert_eq!(h.query_all_by_label_contains("LOSS-REENCODE").count(), 0);
    h.query_all_by_label(words).next().unwrap().hover();
    h.run();
    h.run();
    h.get_by_label_contains("LOSS-REENCODE");

    // A set's layer note (RNM-*).
    h.state_mut().rnm_notes.insert(
        set.key.clone(),
        vec![Warning::new(Code::RnmNoBase, "layer has no base")],
    );
    h.state_mut().select_set(&set.key);
    h.run();
    h.run();
    let words = code_sentence(Code::RnmNoBase);
    assert!(h.query_all_by_label(words).count() >= 1, "{words}");
    assert_eq!(h.query_all_by_label_contains("RNM-NO-BASE").count(), 0);
}

/// C12 in the settings grid: the Alpha combo's options carry the same hints as the Decide… menu.
#[test]
fn the_alpha_combo_options_have_hovers() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    // Decided, so the question box is gone and `Data` can only be the combo's option.
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Alpha(engine::config::AlphaPolicy::Cutout),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.select_item("landscape/leaf.png");
    let mut h = harness(m);
    h.run();
    h.run();
    assert_eq!(h.query_all_by_label("Data").count(), 0);
    // A combo's accessible text is its value.
    h.query_all_by_value("Cutout")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox)
        .expect("the Alpha combo")
        .click();
    h.run();
    let hint = sdc_gui::ui::settings_panel::alpha_hint(engine::config::AlphaPolicy::Data);
    h.get_by_label("Data").hover();
    h.run();
    h.run();
    h.get_by_label(hint);
}
