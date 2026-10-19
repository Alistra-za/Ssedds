//! Layout v2 A5 (R15): the set cards in the Sets column. One row for the handle, the name, the status pill and the
//! ⚠ button; a full-width shader; slot chips labelled by their suffix letter.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::sets::{set_views, slot_row, SlotFill, SlotKind};
use sdc_gui::model::AppModel;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01";

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .with_step_dt(0.02)
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

fn shapes(h: &Harness<'static, AppModel>) -> Vec<egui::Shape> {
    fn flat(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = vec![];
    for c in &h.output().shapes {
        flat(&c.shape, &mut out);
    }
    out
}

/// rock01's name on its card.
fn name_rect(h: &Harness<'static, AppModel>) -> egui::Rect {
    name_of(h, ROCK)
}

/// Set `id`'s name on its card: the leftmost label (the Sets column is left of the set editor).
fn name_of(h: &Harness<'static, AppModel>, id: &str) -> egui::Rect {
    h.query_all_by_label(id)
        .map(|n| n.rect())
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .unwrap_or_else(|| panic!("{id}'s card name"))
}

/// rock01's card.
fn card_rect(h: &Harness<'static, AppModel>) -> egui::Rect {
    card_of(h, ROCK)
}

/// A card: the smallest painted rect holding the set's name and the chip row below it.
fn card_of(h: &Harness<'static, AppModel>, id: &str) -> egui::Rect {
    let name = name_of(h, id);
    shapes(h)
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) => Some(r.rect),
            _ => None,
        })
        .filter(|r| r.contains_rect(name) && r.height() > name.height() + 40.0)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the card's frame")
}

/// The nodes labelled `label` inside rock01's card (K11: every card has a "D").
fn on_card<'a>(h: &'a Harness<'static, AppModel>, label: &'a str) -> Vec<egui_kittest::Node<'a>> {
    on_card_of(h, ROCK, label)
}

fn on_card_of<'a>(
    h: &'a Harness<'static, AppModel>,
    id: &str,
    label: &'a str,
) -> Vec<egui_kittest::Node<'a>> {
    let card = card_of(h, id);
    h.query_all_by_label(label)
        .filter(|n| card.contains_rect(n.rect()))
        .collect()
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

#[test]
fn a_card_shows_name_pill_and_a_full_width_shader() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    h.run();
    let card = card_rect(&h);
    let kind = on_card(&h, "PBR Landscape");
    assert_eq!(kind.len(), 1, "the kind text");
    let (name, kind) = (name_rect(&h), kind[0].rect());
    assert!(
        kind.y_range().contains(name.center().y) && kind.min.x > name.max.x,
        "one row at the default width: the pill {kind:?} right of the name {name:?}"
    );
    let combo = h
        .query_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|n| card.contains_rect(n.rect()))
        .expect("the card's shader");
    assert_eq!(
        combo.accesskit_node().value().as_deref(),
        Some("PBR Landscape (auto)")
    );
    let w = combo.rect().width();
    assert!(
        (card.width() - w).abs() <= 20.0,
        "the shader ({w}) spans the card ({})",
        card.width()
    );
    let texts: Vec<_> = shapes(&h)
        .into_iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) if t.galley.job.text.starts_with("PBR Landscape (") => Some(t),
            _ => None,
        })
        .collect();
    assert!(!texts.is_empty(), "the shader's text is painted");
    assert!(
        texts.iter().all(|t| !t.galley.elided),
        "the shader's text is not cut to \"PBR Landsc…\""
    );
}

#[test]
fn slot_chips_are_labelled_and_empty_ones_are_dim() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened_plain(d.path());
    // A4: the plain PBR shader uses Height and the fixture has no `_p`.
    let v = set_views(&m).into_iter().find(|v| v.id == ROCK).unwrap();
    let row = slot_row(v.shader.as_ref(), &v.slots);
    assert!(
        row.iter()
            .any(|c| c.kind == SlotKind::Height && c.fill == SlotFill::Empty),
        "{row:?}"
    );
    let mut h = harness(m);
    h.run();
    for l in ["D", "N", "RMAOS", "P"] {
        assert_eq!(on_card(&h, l).len(), 1, "one {l:?} chip on rock01's card");
    }
    assert!(h.query_by_label_contains("Height _p (empty)").is_none());
    on_card(&h, "P")[0].hover();
    h.run();
    h.run();
    h.get_by_label("Height _p (empty)");
}

#[test]
fn the_open_set_is_highlighted() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let key = set_views(&m)
        .into_iter()
        .find(|v| v.id == ROCK)
        .unwrap()
        .key;
    let mut h = harness(m);
    let fill = h
        .ctx
        .global_style()
        .visuals
        .selection
        .bg_fill
        .gamma_multiply(0.35);
    let lit = |h: &Harness<'static, AppModel>| {
        let name = name_rect(h);
        shapes(h).iter().any(
            |s| matches!(s, egui::Shape::Rect(r) if r.fill == fill && r.rect.contains_rect(name)),
        )
    };
    h.run();
    assert!(!lit(&h), "a set that is not open is not highlighted");
    h.state_mut().select_set(&key);
    h.run();
    assert!(
        lit(&h),
        "the open set's card is filled with the selection colour"
    );
}

#[test]
fn a_chip_keeps_the_member_behaviours() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let diffuse = m
        .items
        .iter()
        .find(|i| i.name == "rock01.png")
        .unwrap()
        .key
        .clone();
    let mut h = harness(m);
    h.run();
    let at = on_card(&h, "D")[0].rect().center();
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
    }
    h.run();
    assert_eq!(h.state().expanded.as_deref(), Some(diffuse.as_str()));
    h.state_mut().expanded = None;
    h.run();
    on_card(&h, "D")[0].click_secondary();
    h.run();
    h.get_by_label("Slot 1: Diffuse");
}

#[test]
fn the_long_badge_text_is_on_the_pill_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.edit_docs("t", None, true, |_, s| {
        s.add_member(ROCK, "landscape/gone.png")
    });
    idle(&mut m);
    let mut h = harness(m);
    h.run();
    use sdc_gui::model::sets::BADGE_PBR_LANDSCAPE_PGP_ONLY;
    assert!(h
        .query_by_label_contains(BADGE_PBR_LANDSCAPE_PGP_ONLY)
        .is_none());
    on_card(&h, "PBR Landscape")[0].hover();
    h.run();
    h.run();
    h.get_by_label_contains(BADGE_PBR_LANDSCAPE_PGP_ONLY);
    // The recipe and missing-entry lines follow on the same hover.
    h.get_by_label_contains("rmaos: run");
    h.get_by_label_contains("no file matches landscape/gone.png");
}

/// K12: an `add` entry that matches no file is a red "?" chip with the reason on its hover.
#[test]
fn a_missing_entry_is_a_question_mark_chip() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.edit_docs("t", None, true, |_, s| {
        s.add_member(ROCK, "landscape/gone.png")
    });
    idle(&mut m);
    let mut h = harness(m);
    h.run();
    let q = on_card(&h, "?");
    assert_eq!(q.len(), 1, "one chip per missing entry");
    q[0].hover();
    h.run();
    h.run();
    h.get_by_label("no file matches landscape/gone.png");
}

/// Press on `at` and move past the drag threshold: whether a texture is now in flight. The payload is cleared before
/// the release, so nothing is dropped.
fn drags_a_texture(h: &mut Harness<'static, AppModel>, at: egui::Pos2) -> bool {
    use sdc_gui::ui::sets_column::DragItem;
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.hover_at(at);
    h.run();
    h.event(press(true));
    h.run();
    for step in 1..=4 {
        h.event(egui::Event::PointerMoved(
            at + egui::vec2(6.0 * step as f32, 0.0),
        ));
        h.run();
    }
    let dragging = egui::DragAndDrop::payload::<DragItem>(&h.ctx).is_some();
    egui::DragAndDrop::clear_payload(&h.ctx);
    h.event(press(false));
    h.run();
    dragging
}

/// K13: a borrowed chip is outlined amber, says where it is from in its one hover, and is never offered for
/// dragging (dragging it would move another set's member). An own chip still drags.
#[test]
fn a_borrowed_chip_is_amber_and_offers_no_drag() {
    use sdc_gui::model::sets::DropChoice;
    use sdc_gui::model::sets::DropOutcome;
    let d = tempfile::tempdir().unwrap();
    // pbr_export.rs::vanilla_borrow: a new set from leaf borrows rock01's normal.
    let mut m = pbr_ready(d.path());
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", serde_json::json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    let normal = m
        .items
        .iter()
        .find(|i| i.name == "rock01_normal.png")
        .unwrap()
        .key
        .clone();
    assert!(matches!(
        m.drop_choice(&normal, &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    let v = set_views(&m).into_iter().find(|v| v.id == id).unwrap();
    assert!(
        matches!(
            v.slots.get(&SlotKind::Normal),
            Some(SlotFill::Borrowed { .. })
        ),
        "{:?}",
        v.slots
    );
    let mut h = harness(m);
    h.run();
    let chip = on_card_of(&h, &id, "N");
    assert_eq!(chip.len(), 1, "the borrower's N chip");
    let at = chip[0].rect();
    let amber = |h: &Harness<'static, AppModel>, r: egui::Rect| {
        shapes(h).iter().any(|s| {
            matches!(s, egui::Shape::Rect(x) if x.rect == r
                && x.stroke.width == 2.0
                && x.stroke.color == sdc_gui::ui::status::AMBER)
        })
    };
    assert!(amber(&h, at), "the borrowed chip is outlined amber");
    let own = on_card(&h, "N")[0].rect();
    assert!(!amber(&h, own), "rock01's own N chip is not");
    h.hover_at(at.center());
    h.run();
    h.run();
    assert_eq!(
        h.query_all_by_label_contains("borrowed from landscape/rock01")
            .count(),
        1,
        "one hover"
    );
    // The member's name and the borrowed line are the same hover.
    h.get_by_label_contains("rock01_normal.png\nborrowed from landscape/rock01");
    assert!(
        !drags_a_texture(&mut h, at.center()),
        "a borrowed chip offers no drag"
    );
    assert!(
        drags_a_texture(&mut h, own.center()),
        "an own chip still drags"
    );
}

/// Final review I1: a texture dragged over a card outlines that card, on top of the card's own frame (the drop
/// zone's own hover stroke sits under the card frame).
#[test]
fn a_texture_dragged_over_a_card_outlines_it() {
    use sdc_gui::ui::sets_column::DragItem;
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let item = m.items[0].key.clone();
    let mut h = harness(m);
    h.run();
    let card = card_rect(&h);
    let active = h.ctx.global_style().visuals.widgets.active.bg_stroke;
    let near = |a: egui::Rect, b: egui::Rect| {
        (a.min - b.min).length() < 1.0 && (a.max - b.max).length() < 1.0
    };
    // The last rect shape on the card, and whether it is the drop outline.
    let top_is_outline = |h: &Harness<'static, AppModel>| {
        shapes(h)
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if near(r.rect, card) && r.stroke.width > 0.0 => {
                    Some(r.stroke)
                }
                _ => None,
            })
            .next_back()
            .is_some_and(|s| s == active)
    };
    assert!(!top_is_outline(&h), "no outline without a drag");
    let at = card.center();
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.hover_at(at);
    h.run();
    h.event(press(true));
    for step in 1..=3 {
        egui::DragAndDrop::set_payload(&h.ctx, DragItem(item.clone()));
        h.event(egui::Event::PointerMoved(
            at + egui::vec2(2.0 * step as f32, 0.0),
        ));
        h.run();
    }
    assert!(
        egui::DragAndDrop::has_payload_of_type::<DragItem>(&h.ctx),
        "a texture is in flight"
    );
    assert!(
        top_is_outline(&h),
        "the card under a dragged texture is outlined on top of its frame"
    );
    egui::DragAndDrop::clear_payload(&h.ctx);
    h.event(press(false));
    h.run();
    assert!(!top_is_outline(&h), "the outline goes with the drag");
}
