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
    harness_with(d, collapsed, w, three_d, |_| {})
}

fn harness_with(
    d: &std::path::Path,
    collapsed: bool,
    w: f32,
    three_d: bool,
    prep: impl FnOnce(&mut AppModel),
) -> H {
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
    prep(&mut m);
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

/// AG87 (owner: "Keep 3D up", fix round 2): with 3D on, any set member shows its set's 3D view with its settings
/// folded above it, another set's member too; 2D brings the normal texture view back.
#[test]
fn a_member_keeps_the_3d_view_and_2d_shows_its_normal_view() {
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(d.path(), false, 1400.0, true);
    h.get_by_label(FRAME_LABEL);
    let m = &h.state().0;
    let set = sdc_gui::model::set_preview::followed_set(m).unwrap();
    let member = m
        .items
        .iter()
        .find(|i| i.set_key.as_deref() == Some(set.as_str()))
        .unwrap();
    let (key, name) = (member.key.clone(), member.name.clone());
    let other = m
        .items
        .iter()
        .find(|i| i.set_key.is_some() && i.set_key.as_deref() != Some(set.as_str()))
        .map(|i| (i.key.clone(), i.name.clone()));
    h.state_mut().0.selection = Selection::Item(key);
    h.run_steps(3);
    h.get_by_label(FRAME_LABEL);
    h.get_by_label(&format!("{name} settings"));
    h.get_by_label("3D");
    if let Some((k, n)) = other {
        h.state_mut().0.selection = Selection::Item(k);
        h.run_steps(3);
        h.get_by_label(FRAME_LABEL);
        h.get_by_label(&format!("{n} settings"));
    }
    h.state_mut().0.settings.set_view_3d = false;
    h.run_steps(3);
    assert!(h.query_by_label(FRAME_LABEL).is_none(), "2D: no 3D view");
}

/// Fix round 1, item 4: the coat group (under Multilayer, Note AJ) is in the 3D layout's TruePBR settings too.
#[test]
fn the_3d_layout_truepbr_settings_show_the_coat_group() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness_with(d.path(), false, 1400.0, true, |m| {
        m.set_json_value(
            "landscape/rock01",
            "multilayer",
            Some(serde_json::json!(true)),
        );
    });
    h.get_by_label("TruePBR settings").click();
    h.run_steps(3);
    h.get_by_label("Coat settings");
    h.get_by_label("Coat colour");
    h.get_by_label(FRAME_LABEL);
}

/// Fix round 2 (owner): with 3D on, a member clicked straight from the Textures column opens its set's 3D view,
/// even though the set itself was never selected.
#[test]
fn a_member_selected_first_opens_its_sets_3d_view() {
    use sdc_gui::model::Selection;
    let d = tempfile::tempdir().unwrap();
    let mut name = String::new();
    let h = harness_with(d.path(), false, 1400.0, true, |m| {
        let set = sdc_gui::model::set_preview::followed_set(m).unwrap();
        let i = m
            .items
            .iter()
            .find(|i| i.set_key.as_deref() == Some(set.as_str()))
            .unwrap();
        name = i.name.clone();
        m.selection = Selection::Item(i.key.clone());
    });
    h.get_by_label(FRAME_LABEL);
    h.get_by_label(&format!("{name} settings"));
}
