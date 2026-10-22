//! Item 18 (Task 12): "Fill from names…" proposes the files of a set's folder that share its base name and carry a
//! suffix the engine knows, and applies the ticked ones as one step.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::sets::{fill_proposals, set_views, FillProposal, SlotKind};
use sdc_gui::model::AppModel;
use sdc_gui::ui::sets_column::Pending;
use sdc_gui::ui::State;
use std::time::Duration;
use support::*;

const SET: &str = "a/wall";

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

fn members(m: &AppModel) -> Vec<String> {
    set_views(m)
        .into_iter()
        .find(|v| v.id == SET)
        .unwrap()
        .members
}

/// a/wall with its `_g` and `_p` moved out, its normal replaced by brick_n.png, and its Height slot locked.
fn fixture(dir: &std::path::Path) -> AppModel {
    let a = dir.join("textures/a");
    std::fs::create_dir_all(&a).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([(x * 16) as u8, (y * 16) as u8, 90u8]))
        .save(a.join("wall.png"))
        .unwrap();
    for n in ["wall_n.png", "brick_n.png"] {
        ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
            .save(a.join(n))
            .unwrap();
    }
    ImageBuffer::from_fn(16, 16, |x, _| Rgb([(x * 16) as u8, 0, 0]))
        .save(a.join("wall_g.png"))
        .unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Luma([(x * 16) as u8]))
        .save(a.join("wall_p.png"))
        .unwrap();
    let mut m = opened_as_the_app(dir.join("textures"), dir.join("export"));
    m.remove_member(SET, "a/wall_g.png").unwrap();
    idle(&mut m);
    m.remove_member(SET, "a/wall_p.png").unwrap();
    idle(&mut m);
    m.drop_replace("a/brick_n.png", SET, sdc_gui::model::sets::DropChoice::Move)
        .unwrap();
    idle(&mut m);
    m.set_slot_lock(SET, 4, true).unwrap();
    idle(&mut m);
    m
}

fn rows(p: &[FillProposal]) -> Vec<(&str, SlotKind, Option<&str>, Option<&str>)> {
    p.iter()
        .map(|r| {
            (
                r.item.as_str(),
                r.slot,
                r.replaces.as_deref(),
                r.blocked.as_deref(),
            )
        })
        .collect()
}

#[test]
fn proposals_come_from_the_engine_suffixes_with_replace_and_locked_rows() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path());
    assert!(members(&m).contains(&"a/brick_n.png".to_string()));
    // `_g`, `_n`, `_p` are engine suffixes (engine::group::SUFFIXES); brick_n.png has another base name.
    assert!(engine::group::SUFFIXES.contains(&"g") && engine::group::SUFFIXES.contains(&"p"));
    assert_eq!(
        rows(&fill_proposals(&m, SET)),
        vec![
            ("a/wall_g.png", SlotKind::Emissive, None, None),
            ("a/wall_n.png", SlotKind::Normal, Some("brick_n.png"), None),
            ("a/wall_p.png", SlotKind::Height, None, Some("locked")),
        ]
    );
}

#[test]
fn only_the_ticked_rows_apply_as_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    let undo = m.history.undo.len();
    // The locked row is never applied, ticked or not.
    let n = m
        .fill_from_names(SET, &["a/wall_n.png".into(), "a/wall_p.png".into()])
        .unwrap();
    assert_eq!(n, 1);
    idle(&mut m);
    let mem = members(&m);
    assert!(mem.contains(&"a/wall_n.png".to_string()));
    assert!(!mem.contains(&"a/brick_n.png".to_string()), "replaced");
    assert!(!mem.contains(&"a/wall_g.png".to_string()), "not ticked");
    assert!(!mem.contains(&"a/wall_p.png".to_string()), "locked");
    assert_eq!(m.history.undo.len(), undo + 1);
    m.undo();
    idle(&mut m);
    assert!(members(&m).contains(&"a/brick_n.png".to_string()));
    assert!(m.fill_from_names(SET, &["a/wall_p.png".into()]).is_err());
}

/// a/wall (diffuse only) with a `_cnr` and an `_f` beside it, neither yet a member (`_g` and `_sk` share slot 3 the
/// same way, but `_sk` needs a skin set, so this pins the slot 7 pair).
fn clash_fixture(dir: &std::path::Path) -> AppModel {
    let a = dir.join("textures/a");
    std::fs::create_dir_all(&a).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([(x * 16) as u8, (y * 16) as u8, 90u8]))
        .save(a.join("wall.png"))
        .unwrap();
    for n in ["wall_cnr.png", "wall_f.png"] {
        ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
            .save(a.join(n))
            .unwrap();
    }
    let mut m = opened_as_the_app(dir.join("textures"), dir.join("export"));
    for n in ["wall_cnr", "wall_f"] {
        let k = format!("a/{n}.png");
        if members(&m).contains(&k) {
            m.remove_member(SET, &k).unwrap();
            idle(&mut m);
        }
    }
    m
}

#[test]
fn a_batch_never_adds_both_cnr_and_f() {
    let d = tempfile::tempdir().unwrap();
    let m = clash_fixture(d.path());
    let p = fill_proposals(&m, SET);
    let blocked = |n: &str| {
        p.iter()
            .find(|r| r.name == n)
            .and_then(|r| r.blocked.clone())
    };
    let free = |a: &str, b: &str| blocked(a).is_none() as u8 + blocked(b).is_none() as u8;
    assert_eq!(free("wall_cnr.png", "wall_f.png"), 1, "{p:?}");
}

fn harness(m: AppModel) -> Harness<'static, (AppModel, State)> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, (model, state): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, model, state),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn open(h: &mut Harness<'static, (AppModel, State)>) {
    let p = fill_proposals(&h.state().0, SET);
    h.state_mut().1.drops.pending = Some(Pending::FillNames {
        set: SET.into(),
        ticks: vec![true; p.len()],
        rows: p,
    });
    h.run();
    h.run();
}

#[test]
fn the_dialog_lists_rows_esc_cancels_and_enter_applies() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(fixture(d.path()));
    h.run();
    open(&mut h);
    h.get_by_label_contains("Fill a/wall from names");
    h.get_by_label_contains("replaces brick_n.png");
    h.get_by_label_contains("locked");
    h.key_press(eframe::egui::Key::Escape);
    h.run();
    assert!(h.state().1.drops.pending.is_none());
    assert!(members(&h.state().0).contains(&"a/brick_n.png".to_string()));
    open(&mut h);
    // Untick wall_g.png, then Enter applies the rest.
    if let Some(Pending::FillNames { ticks, .. }) = &mut h.state_mut().1.drops.pending {
        ticks[0] = false;
    }
    h.run();
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert!(h.state().1.drops.pending.is_none());
    idle(&mut h.state_mut().0);
    let mem = members(&h.state().0);
    assert!(mem.contains(&"a/wall_n.png".to_string()));
    assert!(!mem.contains(&"a/wall_g.png".to_string()));
}
