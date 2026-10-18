//! A framed collapsing group whose header row carries a value right-aligned (Marmoset style, layout v2 A3).
use eframe::egui;
use egui::collapsing_header::CollapsingState;

/// `value` sits right-aligned in the header. K15: egui toggles on the icon only, so the title toggles too.
pub fn group<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    title: &str,
    value: Option<&str>,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::Frame::group(ui.style())
        .show(ui, |ui| {
            // shortcut: the Debug bound of egui ids is dodged by hashing to a u64 first.
            let id = ui.make_persistent_id(std::hash::BuildHasher::hash_one(&std::hash::BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default(), id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
            let mut clicked = false;
            let mut h = state.show_header(ui, |ui| {
                let label = egui::Label::new(egui::RichText::new(title).strong())
                    .sense(egui::Sense::click());
                clicked = ui.add(label).clicked();
                if let Some(v) = value {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(v)
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
