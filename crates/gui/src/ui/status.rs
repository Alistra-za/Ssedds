//! Status colours (layout v2 A12), one source: red blocks export, amber warns or needs a decision, green is ready.
//! Teal (`visuals.selection`) is the only non-status colour.
use eframe::egui;

pub const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
pub const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(90, 170, 90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Blocks,
    Warning,
    Ready,
}

impl Status {
    fn color(self) -> egui::Color32 {
        match self {
            Status::Blocks => RED,
            Status::Warning => AMBER,
            Status::Ready => GREEN,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Status::Blocks => "blocks export",
            Status::Warning => "warning",
            Status::Ready => "ready",
        }
    }
}

/// An 8 px filled circle in the status colour.
pub fn dot(ui: &mut egui::Ui, s: Status) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, s.color());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, s.label()));
    r
}
