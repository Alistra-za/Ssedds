//! Note FA (trace finding 3): the rename dialog plans once per change, in time that grows with the selection, and draws
//! only the rows on screen. The models are synthetic (cloned items, no files on disk), so ten thousand rows are cheap.
mod support;
use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::rename::{plan_calls, plan_rename, RenameDialog, RenameMode};
use sdc_gui::model::sets::views;
use sdc_gui::model::{AppModel, Dialog};
use std::time::{Duration, Instant};
use support::*;

const LEAF: &str = "landscape/leaf.png";
/// A long shared folder, as a mod's own tree has, so a key compare costs what it costs in real life.
const DIR: &str = "landscape/terrain/ground/rocks/granite/boulders";

/// `opened` with `n` single-texture sets: every item is a clone of the leaf diffuse under its own key and output.
fn synthetic(dir: &std::path::Path, n: usize) -> AppModel {
    let mut m = opened(dir);
    let item = m.item(LEAF).expect("leaf").clone();
    let set = m
        .sets
        .iter()
        .find(|s| s.members.iter().any(|k| k == LEAF))
        .expect("leaf's set")
        .clone();
    m.items = (0..n)
        .map(|i| {
            let mut it = item.clone();
            it.key = format!("{DIR}/s{i:05}.png");
            it.job_key = format!("{DIR}/s{i:05}.dds");
            it
        })
        .collect();
    m.sets = (0..n)
        .map(|i| {
            let mut s = set.clone();
            s.key = format!("{}{i:05}", set.key);
            s.members = vec![format!("{DIR}/s{i:05}.png")];
            s
        })
        .collect();
    m.invalidate_views();
    assert_eq!(views(&m).len(), n);
    m
}

fn dialog_over(m: &AppModel, mode: RenameMode) -> RenameDialog {
    let keys = match mode {
        RenameMode::Inputs => m.items.iter().map(|i| i.key.clone()).collect(),
        RenameMode::Outputs => views(m).iter().map(|v| v.id.clone()).collect(),
    };
    let mut d = RenameDialog {
        mode,
        keys,
        ..RenameDialog::default()
    };
    d.recipe.prefix = "x_".into();
    d
}

/// The best of `runs` plans, so one slow run (a scheduler hiccup) does not decide.
fn best_plan(m: &AppModel, d: &RenameDialog, runs: usize) -> Duration {
    (0..runs)
        .map(|_| {
            let t = Instant::now();
            let p = plan_rename(m, d);
            let e = t.elapsed();
            assert_eq!(p.rows.len(), d.keys.len());
            e
        })
        .min()
        .unwrap()
}

/// The rows are what the recipe says for every row, not only for a small selection.
fn assert_rows(m: &AppModel, d: &RenameDialog) {
    let p = plan_rename(m, d);
    assert!(p.error.is_none() && p.can_apply());
    for (i, r) in p.rows.iter().enumerate() {
        assert_eq!(r.problem, None, "row {i}");
        assert_eq!(r.skipped, None, "row {i}");
        assert!(r.included && r.writes() && r.matched, "row {i}");
        assert_eq!(r.warning, None, "row {i}");
        match d.mode {
            RenameMode::Inputs => {
                assert_eq!(r.key, format!("{DIR}/s{i:05}.png"));
                assert_eq!(r.new, format!("{DIR}/x_s{i:05}.png"));
                assert_eq!(r.output, format!("{DIR}/x_s{i:05}.dds"));
            }
            RenameMode::Outputs => {
                assert_eq!(r.old, format!("s{i:05}"));
                assert_eq!(r.new, format!("x_s{i:05}"));
                assert_eq!(r.output, format!("{DIR}/x_s{i:05}.dds"));
            }
        }
    }
}

#[test]
fn plan_rename_scales_linearly() {
    let d = tempfile::tempdir().unwrap();
    // Four times the rows must cost well under 16 times (a quadratic plan), with room for noise: a linear plan gives
    // about 4, an n log n one a little more.
    const N: usize = 1000;
    let (small, large) = (synthetic(d.path(), N), synthetic(d.path(), 4 * N));
    for mode in [RenameMode::Inputs, RenameMode::Outputs] {
        let (ds, dl) = (dialog_over(&small, mode), dialog_over(&large, mode));
        assert_rows(&small, &ds);
        assert_rows(&large, &dl);
        let (ts, tl) = (best_plan(&small, &ds, 5), best_plan(&large, &dl, 3));
        eprintln!("{mode:?}: {N} rows {ts:?}, {} rows {tl:?}", 4 * N);
        assert!(
            tl < ts * 8,
            "{mode:?}: planning {} rows took {tl:?}, more than 8 times the {ts:?} of {N} rows",
            4 * N
        );
    }
}

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::dialogs::draw(ui.ctx(), model),
            model,
        )
}

#[test]
fn rename_dialog_draws_only_visible_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = synthetic(d.path(), 10_000);
    m.select_all_visible();
    m.open_rename(RenameMode::Inputs);
    let Some(Dialog::Rename(dialog)) = &m.dialog else {
        panic!("the rename dialog is open");
    };
    assert_eq!(dialog.keys.len(), 10_000);
    let mut h = harness(m);
    h.run_steps(4);
    let drawn = h.query_all_by_label_contains(".png").count();
    assert!(drawn > 0, "the rows are drawn");
    assert!(drawn < 100, "{drawn} of 10000 rows were laid out");
}

#[test]
fn draw_reuses_the_plan_when_nothing_changed() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.select_all_visible();
    m.open_rename(RenameMode::Inputs);
    let before = plan_calls();
    // Building the harness draws a first frame.
    let mut h = harness(m);
    h.run_steps(5);
    assert_eq!(plan_calls() - before, 1, "five idle frames plan once");
    if let Some(Dialog::Rename(d)) = h.state_mut().dialog.as_mut() {
        d.recipe.prefix = "x_".into();
    }
    h.run_steps(5);
    assert_eq!(plan_calls() - before, 2, "an edit plans again, once");
    assert!(h.state_mut().apply_rename());
    assert_eq!(plan_calls() - before, 3, "Apply plans for itself");
}
