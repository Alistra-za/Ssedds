//! Status colours (layout v2 A12), one source: red blocks export, amber warns or is undecided, green is ready.
//! Teal (`visuals.selection`) is the only non-status colour. Checks and Info rows are not a status (Note FC3): their
//! dot is the weak text colour.
use eframe::egui;

pub const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
pub const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(90, 170, 90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Blocks,
    Warning,
    Ready,
    /// Note FC3 (amends CL18): an artist check, not amber and not a ready signal.
    Check,
    /// Note FC3: a result with an action and no consequence for the export.
    Info,
}

impl Status {
    fn color(self, ui: &egui::Ui) -> egui::Color32 {
        match self {
            Status::Blocks => RED,
            Status::Warning => AMBER,
            Status::Ready => GREEN,
            Status::Check | Status::Info => ui.visuals().weak_text_color(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Status::Blocks => "blocks export",
            Status::Warning => "warning",
            Status::Ready => "ready",
            Status::Check => "check",
            Status::Info => "info",
        }
    }

    /// Whether a row of this status draws its sentence weak (Note FC3).
    pub fn weak(self) -> bool {
        matches!(self, Status::Check | Status::Info)
    }
}

/// An 8 px filled circle in the status colour.
pub fn dot(ui: &mut egui::Ui, s: Status) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, s.color(ui));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, s.label()));
    r
}

/// The hover of a line that shows a code's sentence or an Info message (Note FB8): the engine's message, then the
/// code in weak text. The one place a code is printed in the panels.
pub fn code_hover(ui: &mut egui::Ui, message: &str, codes: &str) {
    ui.label(message);
    ui.weak(codes);
}
