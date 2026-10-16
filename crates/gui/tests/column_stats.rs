//! Audit 3 item 7: the Textures column's per-frame counts are built again only when something they read changed.
use eframe::egui;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::columns::column_stats;
use std::sync::Arc;
mod support;

#[test]
fn column_stats_are_rebuilt_only_when_the_inputs_change() {
    let ctx = egui::Context::default();
    let mut m = AppModel::placeholder(GuiSettings::default());
    let a = column_stats(&ctx, &m);
    assert!(
        Arc::ptr_eq(&a, &column_stats(&ctx, &m)),
        "nothing changed: shared"
    );
    m.select_item("x");
    let b = column_stats(&ctx, &m);
    assert!(!Arc::ptr_eq(&a, &b), "a new selection");
    assert!(b.chosen.contains("x"));
    m.search = "q".into();
    let c = column_stats(&ctx, &m);
    assert!(!Arc::ptr_eq(&b, &c), "a new search");
    m.filter.failed = true;
    assert!(!Arc::ptr_eq(&c, &column_stats(&ctx, &m)), "a new filter");
    let d = column_stats(&ctx, &m);
    m.invalidate_views();
    assert!(!Arc::ptr_eq(&d, &column_stats(&ctx, &m)), "new items");
}

/// Follow-up: every selection change bumps `multi.selection_rev`, and the cached summary follows it.
#[test]
fn the_summary_follows_every_selection_change() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let base = m.items[0].clone();
    m.items = (0..6)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/t{n}_{}", if n % 2 == 0 { "keep" } else { "drop" });
            i
        })
        .collect();
    m.invalidate_views();
    m.search = "keep".into();
    let ctx = egui::Context::default();
    let k = |n: usize| m.items[n].key.clone();
    let (k0, k1, k2, k3, k4) = (k(0), k(1), k(2), k(3), k(4));
    let mut rev = m.multi.selection_rev;
    let mut check = |m: &mut AppModel, want: Option<&str>| {
        assert!(m.multi.selection_rev > rev, "bumped");
        rev = m.multi.selection_rev;
        assert_eq!(column_stats(&ctx, m).summary.as_deref(), want);
    };
    m.click_item(&k0, false, false);
    check(&mut m, None);
    m.click_item(&k1, true, false); // hidden by the search
    check(&mut m, Some("2 selected (1 hidden by filter)"));
    m.click_item(&k2, true, false);
    check(&mut m, Some("3 selected (1 hidden by filter)"));
    m.click_item(&k1, true, false); // the middle one leaves
    check(&mut m, Some("2 selected"));
    m.click_item(&k3, true, false);
    check(&mut m, Some("3 selected (1 hidden by filter)"));
    m.click_item(&k4, false, false);
    check(&mut m, None);
}
