//! The bottom panel (layout v2 A1, A9): Files and Messages tabs, its placement under the columns.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Id};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::AppModel;
use sdc_gui::ui::columns::{SETS_MIN, TEXTURES_MIN};
use sdc_gui::ui::set_preview::CENTRE_MIN;
use std::time::Duration;
use support::*;

fn harness_at(model: AppModel, size: egui::Vec2) -> Harness<'static, AppModel> {
    let h = Harness::builder().with_size(size).build_ui_state(
        |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
        model,
    );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    harness_at(model, egui::vec2(1400.0, 900.0))
}

fn outer(h: &Harness<'static, AppModel>, id: &str) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new(id)).unwrap().outer_rect
}

#[test]
fn the_panel_starts_collapsed_with_its_tabs_and_toggling_messages_opens_and_closes_it() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label("Files");
    // The header's Messages button (until branch 3) and the panel's Messages tab.
    assert_eq!(h.query_all_by_label("Messages").count(), 2);
    assert!(!h.state().bottom.open);
    h.state_mut().toggle_tab(BottomTab::Messages);
    h.run();
    assert!(h.state().messages.is_open());
    h.get_by_label_contains("messages"); // "0 messages"
    h.state_mut().toggle_tab(BottomTab::Messages);
    h.run();
    assert!(!h.state().bottom.open && !h.state().messages.is_open());
}

#[test]
fn an_export_opens_the_files_tab() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert!(m.bottom.open && m.bottom.tab == BottomTab::Files);
}

#[test]
fn the_panel_spans_the_window_under_the_columns() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.state_mut().show_tab(BottomTab::Files);
    h.run_steps(6);
    // The harness insets the root ui by 8 px; the header spans that width, so the panel must match it.
    let (p, t, top) = (
        outer(&h, "bottom"),
        outer(&h, "textures"),
        outer(&h, "header"),
    );
    assert!(
        (p.left() - top.left()).abs() <= 1.0 && (p.right() - top.right()).abs() <= 1.0,
        "panel {p:?} vs header {top:?}"
    );
    assert!(p.width() > 1300.0, "{p:?}");
    assert!(
        t.bottom() <= p.top() + 0.5,
        "textures {t:?} reaches into {p:?}"
    );
}

#[test]
fn at_1280x800_with_the_panel_open_the_columns_keep_their_minimums() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness_at(opened(d.path()), egui::vec2(1280.0, 800.0));
    h.state_mut().show_tab(BottomTab::Files);
    h.run_steps(6);
    let (t, s) = (outer(&h, "textures"), outer(&h, "sets"));
    assert!(t.width() >= TEXTURES_MIN - 0.5, "textures {}", t.width());
    assert!(s.width() >= SETS_MIN - 0.5, "sets {}", s.width());
    assert!(
        1280.0 - s.right() >= CENTRE_MIN - 0.5,
        "centre {}",
        1280.0 - s.right()
    );
}

// ---- Task 1.6: the Problems tab, click-throughs and green dots ----

/// Every item of a PBR project carries exactly `ws` as its warnings (copy of set_editor_ui.rs, K16).
fn with_only_warnings(ws: &[engine::codes::Warning]) -> (tempfile::TempDir, AppModel) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
    for s in m.sets.iter_mut() {
        s.warnings.clear();
    }
    let key = rock01(&m).members[0].clone();
    m.items.iter_mut().find(|i| i.key == key).unwrap().warnings = ws.to_vec();
    m.invalidate_views();
    (d, m)
}

fn rock01(m: &AppModel) -> sdc_gui::model::sets::SetView {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap()
}

/// `n` undecided textures cloned from a planned one (textures_filter.rs::many, K16).
fn many_undecided(dir: &std::path::Path, n: usize) -> AppModel {
    use engine::codes::Code;
    use sdc_gui::model::items::{Decision, ItemStatus};
    let mut m = opened(dir);
    let base = m.items.iter().find(|i| i.job.is_some()).unwrap().clone();
    m.items = (0..n)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/t{n:05}");
            i.name = format!("t{n:05}.png");
            i.status = ItemStatus::NeedsDecision;
            i.decisions = vec![Decision::Acknowledge(Code::DimNpot)];
            i.warnings = vec![];
            i.set_key = None;
            i
        })
        .collect();
    m.sets = vec![];
    m.invalidate_views();
    m
}

fn idle(h: &mut Harness<'static, AppModel>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

#[test]
fn the_need_a_decision_count_opens_problems_filtered_to_undecided() {
    use sdc_gui::model::bottom::ProblemsFilter;
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label_contains("need a decision").click();
    h.run();
    assert_eq!(h.state().bottom.tab, BottomTab::Problems);
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Undecided));
    h.get_by_label("Undecided only ×");
}

#[test]
fn a_set_badge_opens_problems_filtered_to_its_set() {
    use engine::codes::{Code, Warning};
    use sdc_gui::model::bottom::ProblemsFilter;
    let (_d, m) = with_only_warnings(&[Warning::new(Code::LossReencode, "re-encoded")]);
    let key = rock01(&m).key;
    let mut h = harness(m);
    h.run();
    h.get_by_label("⚠ 1").click();
    h.run();
    assert_eq!(h.state().bottom.tab, BottomTab::Problems);
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(key)));
    h.get_by_label("Set: landscape/rock01 ×");
}

#[test]
fn a_problems_row_selects_its_texture_and_decides_in_place() {
    use engine::codes::Code;
    use sdc_gui::model::items::Decision;
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // The alpha question only follows a sub-type.
    m.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m);
    h.state_mut().open_problems(None);
    h.run();
    h.get_by_label("landscape/odd.png").click();
    h.run();
    assert!(matches!(&h.state().selection, Selection::Item(k) if k == "landscape/odd.png"));
    // Rows follow the item order, so odd is the last row with an Acknowledge.
    let n = h.query_all_by_label("Acknowledge").count();
    assert!(n >= 1);
    h.query_all_by_label("Acknowledge").last().unwrap().click();
    idle(&mut h);
    let has = |h: &Harness<'static, AppModel>, k: &str, f: fn(&Decision) -> bool| {
        h.state().item(k).unwrap().decisions.iter().any(f)
    };
    let ack = |d: &Decision| matches!(d, Decision::Acknowledge(Code::DimNpot));
    assert!(!has(&h, "landscape/odd.png", ack), "odd lost its question");
    let alpha = |d: &Decision| matches!(d, Decision::AlphaMeaning);
    assert!(has(&h, "landscape/leaf.png", alpha));
    h.get_by_label("Decide…").click();
    h.run();
    h.get_by_label("Cutout").click();
    idle(&mut h);
    assert!(
        !has(&h, "landscape/leaf.png", alpha),
        "leaf lost its alpha question"
    );
}

#[test]
fn a_read_only_project_disables_the_fix_buttons_with_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    let mut seen = 0;
    for label in ["Set type…", "Decide…", "Acknowledge"] {
        for n in h.query_all_by_label(label) {
            seen += 1;
            assert!(n.accesskit_node().is_disabled(), "{label} is enabled");
        }
    }
    assert!(seen >= 3, "only {seen} fix buttons found");
    assert!(h
        .state()
        .fix_off()
        .unwrap()
        .contains("sdc.toml is read-only"));
}

#[test]
fn the_problems_tab_draws_only_the_rows_on_screen() {
    let d = tempfile::tempdir().unwrap();
    let mut m = many_undecided(d.path(), 2000);
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    let shown = h.query_all_by_label_contains("t/t0").count();
    assert!((1..100).contains(&shown), "{shown} problem links");
    // One build, and the filtered list is not rebuilt per frame.
    let builds = sdc_gui::model::bottom::problems_builds(h.state());
    h.run_steps(3);
    assert_eq!(sdc_gui::model::bottom::problems_builds(h.state()), builds);
}

#[test]
fn a_ready_set_and_tile_show_a_green_dot() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let v = rock01(&m);
    assert!(
        !sdc_gui::model::bottom::set_ready(&m, &v),
        "undecided members"
    );
    decide_all(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(h.query_all_by_label("ready").count() > 0);
    let d2 = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d2.path()));
    h.run();
    let n = h.state().items.len();
    assert!(
        h.query_all_by_label("ready").count() < n,
        "an undecided folder is not all ready"
    );
}

#[test]
fn the_set_editor_points_to_problems_instead_of_listing_codes() {
    use sdc_gui::model::bottom::ProblemsFilter;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let key = rock01(&m).key;
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    assert!(h
        .query_all_by_label_contains("IN-GREEN-UNSTATED:")
        .next()
        .is_none());
    h.get_by_label_contains("in this set — Show in Problems")
        .click();
    h.run();
    assert_eq!(h.state().bottom.filter, Some(ProblemsFilter::Set(key)));
}

#[test]
fn the_status_dot_names_its_state() {
    use sdc_gui::ui::status::{dot, Status};
    let mut h = Harness::new_ui(|ui| {
        for s in [Status::Blocks, Status::Warning, Status::Ready] {
            dot(ui, s);
        }
    });
    h.run();
    for l in ["blocks export", "warning", "ready"] {
        h.get_by_label(l);
    }
}
