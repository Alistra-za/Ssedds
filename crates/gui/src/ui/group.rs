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
                let right = ui.max_rect().right().min(ui.clip_rect().right());
                ui.set_min_width((right - ui.min_rect().left()).max(0.0));
                let label = egui::Label::new(egui::RichText::new(title).strong())
                    .sense(egui::Sense::click());
                clicked = ui.add(label).clicked();
                if let Some(v) = value {
                    let mut r = ui.available_rect_before_wrap();
                    r.max.x = right.max(r.min.x);
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
