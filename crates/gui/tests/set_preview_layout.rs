//! The set's 3D view inside the whole Textures view (no GPU, no scan DB). AG78: the 2D | 3D switch swaps the
//! centre pane in place; there is no side panel, and the 3D image stays inside the centre.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Id};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::set_preview::{SetPreview, DEFAULT_CAP};
use sdc_gui::model::viewport3d::FRAME_LABEL;
use sdc_gui::model::{pbr_export::json_entries, AppModel};
use sdc_gui::ui::set_preview::CENTRE_MIN;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

fn harness(d: &std::path::Path, collapsed: bool, w: f32, three_d: bool) -> H {
    let mut m = pbr_opened(d);
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    m.settings.textures_collapsed = collapsed;
    m.settings.sets_collapsed = collapsed;
    m.settings.set_view_3d = three_d;
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

fn check(w: f32, collapsed: bool) {
    let d = tempfile::tempdir().unwrap();
    let h = harness(d.path(), collapsed, w, true);
    assert!(outer(&h, "set-3d").is_none(), "no side panel at {w}");
    let left = ["textures", "sets", "textures-strip", "sets-strip"]
        .iter()
        .filter_map(|i| outer(&h, i))
        .map(|r| r.max.x)
        .fold(0.0f32, f32::max);
    // The Advanced drawer's strip is the only thing right of the centre.
    let right = outer(&h, "advanced-strip").map_or(w, |r| r.min.x);
    assert!(
        right - left >= CENTRE_MIN - 0.5,
        "centre {} at {w}",
        right - left
    );
    // The frame takes the centre's width (less its margins), between the columns and the strip.
    let img = h.get_by_label(FRAME_LABEL).rect();
    assert!(
        img.min.x >= left - 0.5,
        "image {img:?} over the columns at {left}"
    );
    assert!(
        img.max.x <= right + 0.5,
        "image {img:?} past the centre's {right}"
    );
    assert!(
        img.width() >= (right - left) - 24.0,
        "image {} wide in a centre of {}",
        img.width(),
        right - left
    );
    // The set editor's header row stays above it; its slot cards are gone.
    h.get_by_label("2D");
    assert!(h.query_by_label("Match on this").is_none());
}

#[test]
fn narrow_window_expanded_columns() {
    check(900.0, false);
}

#[test]
fn wide_window_expanded_columns() {
    check(1600.0, false);
}

#[test]
fn narrow_window_collapsed_columns() {
    check(700.0, true);
}

#[test]
fn in_2d_the_centre_is_the_set_editor_and_no_3d_view_exists() {
    let d = tempfile::tempdir().unwrap();
    let h = harness(d.path(), false, 1280.0, false);
    assert!(h.query_all_by_label("Match on this").count() >= 1);
    assert!(h.query_by_label(FRAME_LABEL).is_none());
    assert!(outer(&h, "set-3d").is_none());
}
