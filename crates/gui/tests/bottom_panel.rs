//! The bottom panel (layout v2 A1, A9): Files and Messages tabs, its placement under the columns.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Id};
use egui_kittest::kittest::Queryable;
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
