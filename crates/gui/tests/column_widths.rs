//! The Textures view's resizable columns keep their minimum width, and a divider drag stops at it.
mod support;
use eframe::egui::{self, containers::panel::PanelState, Event, Id, Modifiers, PointerButton};
use egui_kittest::Harness;
use sdc_gui::model::AppModel;
use sdc_gui::ui::columns::{SETS_MIN, STRIP_WIDTH, TEXTURES_MIN};
use sdc_gui::ui::set_preview::CENTRE_MIN;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State)>;

fn harness(d: &std::path::Path, w: f32) -> H {
    let mut m = pbr_opened(d);
    m.settings.columns_collapsed = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(w, 800.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(6);
    h
}

fn outer(h: &H, id: &str) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new(id)).unwrap().outer_rect
}

/// Where the column's divider is drawn this frame. egui does not store a panel's size mid-drag; the handle moves.
fn divider(h: &H, id: &str) -> f32 {
    let r = h.ctx.read_response(Id::new(id).with("__resize")).unwrap();
    r.rect.center().x
}

fn ev(h: &mut H, e: Event) {
    h.input_mut().events.push(e);
    h.step();
}

fn button(pos: egui::Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// Drag column `id`'s divider to `to_x`: its width each frame of the drag, then its stored width after release.
fn drag(h: &mut H, id: &str, to_x: f32) -> (Vec<f32>, f32) {
    let left = outer(h, id).left();
    let from = egui::pos2(divider(h, id), outer(h, id).center().y);
    ev(h, Event::PointerMoved(from));
    ev(h, button(from, true));
    let mut during = vec![];
    for i in 1..=8 {
        let x = from.x + (to_x - from.x) * i as f32 / 8.0;
        ev(h, Event::PointerMoved(egui::pos2(x, from.y)));
        during.push(divider(h, id) - left);
    }
    ev(h, button(egui::pos2(to_x, from.y), false));
    h.run_steps(3);
    (during, outer(h, id).width())
}

#[test]
fn a_narrow_window_keeps_each_column_at_its_minimum() {
    // Room for both minimums beside the Advanced strip, not for the centre's.
    let w = STRIP_WIDTH + TEXTURES_MIN + SETS_MIN + CENTRE_MIN / 2.0;
    let d = tempfile::tempdir().unwrap();
    let h = harness(d.path(), w);
    let (t, s) = (outer(&h, "textures"), outer(&h, "sets"));
    assert!(t.width() >= TEXTURES_MIN - 0.5, "textures {}", t.width());
    assert!(s.width() >= SETS_MIN - 0.5, "sets {}", s.width());
    assert!(
        s.min.x >= t.max.x - 0.5,
        "sets {s:?} overlaps textures {t:?}"
    );
}

#[test]
fn a_window_narrower_than_the_minimums_does_not_go_negative() {
    let d = tempfile::tempdir().unwrap();
    let h = harness(d.path(), STRIP_WIDTH + TEXTURES_MIN / 2.0);
    for id in ["textures", "sets"] {
        let r = outer(&h, id);
        assert!(r.width() >= 0.0 && r.width().is_finite(), "{id} {r:?}");
    }
}

#[test]
fn a_drag_past_the_minimum_stops_at_the_minimum() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(d.path(), 1280.0);
    for (id, min) in [("sets", SETS_MIN), ("textures", TEXTURES_MIN)] {
        let left = outer(&h, id).left();
        let (during, after) = drag(&mut h, id, left + 10.0);
        for w in during {
            assert!(w >= min - 2.0, "{id} went to {w} mid-drag, min {min}");
        }
        assert!(
            after >= min - 0.5 && after <= min + 2.0,
            "{id} released at {after}, min {min}"
        );
    }
}

#[test]
fn a_drag_to_the_far_right_leaves_the_others_their_minimums() {
    let w = 1280.0;
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(d.path(), w);
    let (_, after) = drag(&mut h, "textures", w - 5.0);
    let max = w - STRIP_WIDTH - SETS_MIN - CENTRE_MIN;
    assert!(after <= max + 1.0, "textures {after}, max {max}");
    let s = outer(&h, "sets");
    assert!(s.width() >= SETS_MIN - 0.5, "sets {}", s.width());
}
