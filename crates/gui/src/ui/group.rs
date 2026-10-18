//! A framed collapsing group whose header row carries a value right-aligned (Marmoset style, layout v2 A3).
use eframe::egui;
use egui::collapsing_header::CollapsingState;

/// `value` sits right-aligned in the header, styled by the caller (weak, or AMBER for a warning). K15: egui toggles
/// on the icon only, so the title toggles too.
pub fn group<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    title: &str,
    value: Option<egui::RichText>,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::Frame::group(ui.style())
        .show(ui, |ui| {
            // shortcut: the Debug bound of egui ids is dodged by hashing to a u64 first; upgrade to `make_persistent_id(id)`
            // if egui drops the Debug bound or every caller's id type derives Debug.
            let hasher = std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default();
            let id = ui.make_persistent_id(std::hash::BuildHasher::hash_one(&hasher, id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
            let mut clicked = false;
            let mut h = state.show_header(ui, |ui| {
                // The visible edge: a wider row above widens max_rect (egui unions it into the content), not the panel.
                let inset = ui.spacing().item_spacing.x; // the clip edge cuts the last glyph otherwise
                let right = ui.max_rect().right().min(ui.clip_rect().right() - inset);
                // Placer::set_min_width measures from the cursor, which is past the collapser here.
                ui.set_min_width((right - ui.cursor().min.x).max(0.0));
                let label = egui::Label::new(egui::RichText::new(title).strong())
                    .sense(egui::Sense::click());
                clicked = ui.add(label).clicked();
                // A title wider than the visible width leaves no room: skip the value rather than draw it over the title.
                if let Some(v) = value.filter(|_| ui.cursor().min.x < right) {
                    let mut r = ui.available_rect_before_wrap();
                    r.max.x = right;
                    let layout = egui::Layout::right_to_left(egui::Align::Center);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(layout), |ui| {
                        ui.label(v)
                    });
                }
            });
            if clicked {
                h.toggle();
            }
            h.body(body).2.map(|r| r.inner)
        })
        .inner
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    /// (content width, view width) of a 300 px horizontal ScrollArea holding one group, after a few frames.
    fn widths(value: bool) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut out = (0.0, 0.0);
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 400.0),
                )),
                ..Default::default()
            };
            let full = ctx.run_ui(input, |ui| {
                let o = egui::ScrollArea::horizontal().show(ui, |ui| {
                    super::group(ui, "g", "Coat", value.then(|| "off".into()), false, |_| {});
                });
                out = (o.content_size.x, o.inner_rect.width());
            });
            full.drop_without_applying_deltas();
        }
        out
    }

    /// Re-review of I1: the header row filled from the frame's left edge, not from the cursor after the collapser,
    /// so every group was ~18 px wider than its panel and the panel scrolled sideways.
    #[test]
    fn a_group_header_never_widens_its_scroll_area() {
        for value in [false, true] {
            let (content, view) = widths(value);
            assert!(
                content <= view,
                "value {value}: content {content} > view {view}"
            );
        }
    }
}
