//! Audit 3 item 12: the RNM blended-preview texture is held with its `Arc` and only one is kept.
use eframe::egui;
use sdc_gui::model::rnm_preview::RnmThumb;
use sdc_gui::ui::rnm_section::preview_texture;
use std::sync::Arc;

fn thumb(c: u8) -> Arc<RnmThumb> {
    Arc::new(RnmThumb {
        width: 1,
        height: 1,
        rgba: vec![c, c, c, 255],
    })
}

#[test]
fn the_preview_texture_follows_the_arc_and_frees_the_last_key() {
    let ctx = egui::Context::default();
    let a = thumb(1);
    let ta = preview_texture(&ctx, "a", &a);
    assert_eq!(
        ta.id(),
        preview_texture(&ctx, "a", &a).id(),
        "same Arc: reused"
    );
    assert_eq!(
        Arc::strong_count(&a),
        2,
        "the cache holds the Arc, not a pointer"
    );
    let b = thumb(2);
    let tb = preview_texture(&ctx, "b", &b);
    assert_ne!(ta.id(), tb.id());
    assert_eq!(
        Arc::strong_count(&a),
        1,
        "the previous key's picture is dropped"
    );
    let a2 = thumb(1);
    assert_ne!(
        tb.id(),
        preview_texture(&ctx, "a", &a2).id(),
        "a new Arc uploads again"
    );
}
