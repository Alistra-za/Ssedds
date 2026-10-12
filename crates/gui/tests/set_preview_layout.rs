//! The set preview panel inside the whole Textures view (no GPU, no scan DB): it must not squeeze the columns
//! below their minimum width, and its 3D image must stay inside the panel.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Id};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
use sdc_gui::model::viewport3d::FRAME_LABEL;
use sdc_gui::model::{pbr_export::json_entries, AppModel};
use sdc_gui::ui::set_preview::{CENTRE_MIN, PANEL_MIN};
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

fn harness(d: &std::path::Path, collapsed: bool, w: f32) -> H {
    let mut m = pbr_opened(d);
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    m.settings.columns_collapsed = collapsed;
    let state = State {
        set3d: SetPreview::new(None, None, None, DEFAULT_CAP),
        ..State::default()
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(w, 800.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    for _ in 0..6 {
        h.run_steps(3);
        let (m, s) = h.state_mut();
        s.set3d.poll(&*m);
    }
    h
}

fn outer(h: &H, id: &str) -> Option<egui::Rect> {
    PanelState::load(&h.ctx, Id::new(id)).map(|p| p.outer_rect)
}

fn check(w: f32, collapsed: bool, visible: bool) {
    let d = tempfile::tempdir().unwrap();
    let h = harness(d.path(), collapsed, w);
    let panel = outer(&h, "set-3d");
    let left: f32 = ["textures", "sets", "textures-strip", "sets-strip"]
        .iter()
        .filter_map(|i| outer(&h, i))
        .map(|r| r.width())
        .sum();
    assert!(panel.is_some() || !visible, "panel missing at {w}");
    let Some(panel) = panel else {
        // Hidden: only allowed when the window cannot hold the columns, the minimum centre and the minimum panel.
        assert!(w - left < CENTRE_MIN + PANEL_MIN, "panel hidden at {w}");
        return;
    };
    assert!(
        panel.width() >= PANEL_MIN - 0.5,
        "panel {} at {w}",
        panel.width()
    );
    let centre = w - left - panel.width();
    assert!(
        centre >= CENTRE_MIN - 0.5,
        "centre {centre} at {w}, panel {}",
        panel.width()
    );
    let img = h.get_by_label(FRAME_LABEL).rect();
    assert!(
        img.min.x >= panel.min.x - 0.5 && img.max.x <= panel.max.x + 0.5,
        "image {img:?} outside panel {panel:?}"
    );
}

#[test]
fn narrow_window_expanded_columns() {
    check(900.0, false, false);
}

#[test]
fn wide_window_expanded_columns() {
    check(1600.0, false, true);
}

#[test]
fn the_default_window_expanded_columns() {
    check(1280.0, false, true);
}

#[test]
fn narrow_window_collapsed_columns() {
    check(700.0, true, true);
}
