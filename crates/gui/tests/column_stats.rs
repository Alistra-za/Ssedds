//! Audit 3 item 7: the Textures column's per-frame counts are built again only when something they read changed.
use eframe::egui;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::columns::column_stats;
use std::sync::Arc;

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
