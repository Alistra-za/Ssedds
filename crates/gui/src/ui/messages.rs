//! Item 20 (DECISIONS AG112-AG114): the toasts at the bottom right and the Messages panel. Drawn once a frame by
//! `SdcApp` for both views; the store is `model::messages`.
use super::columns::{AMBER, RED};
use crate::model::messages::{Severity, Toast};
use crate::model::AppModel;
use eframe::egui;
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn color(s: Severity) -> Option<egui::Color32> {
    match s {
        Severity::Info => None,
        Severity::Warning => Some(AMBER),
        Severity::Error => Some(RED),
    }
}

/// The toast rectangles of the last frame: the pointer over one pauses its fade.
#[derive(Clone, Default)]
struct Rects(Vec<(u64, egui::Rect)>);

/// Advances and draws the toasts, then draws the Messages panel when it is open.
pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    draw_at(ctx, model, Instant::now());
    panel(ctx, model);
}

/// `draw`'s toast half at clock `now`.
pub fn draw_at(ctx: &egui::Context, model: &mut AppModel, now: Instant) {
    let id = egui::Id::new("toast-rects");
    let last: Rects = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let pointer = ctx.pointer_hover_pos();
    let hovered: HashSet<u64> = last
        .0
        .iter()
        .filter(|(_, r)| pointer.is_some_and(|p| r.contains(p)))
        .map(|(i, _)| *i)
        .collect();
    let alive = model.messages.tick(now, &hovered);
    let toasts: Vec<Toast> = model.messages.toasts().to_vec();
    let mut rects = Rects::default();
    if !toasts.is_empty() {
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                for t in &toasts {
                    let frame = egui::Frame::popup(ui.style());
                    let r = frame.show(ui, |ui| {
                        ui.set_opacity(t.opacity());
                        match color(t.severity) {
                            Some(c) => ui.colored_label(c, &t.text),
                            None => ui.label(&t.text),
                        }
                    });
                    rects.0.push((t.id, r.response.rect));
                }
            });
    }
    ctx.data_mut(|d| d.insert_temp(id, rects));
    if alive {
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

/// The Messages window: time, severity and text of the last entries, newest first, and a Clear button.
pub fn panel(ctx: &egui::Context, model: &mut AppModel) {
    let mut open = model.messages.is_open();
    if !open {
        return;
    }
    let mut clear = false;
    egui::Window::new("Messages")
        .open(&mut open)
        .default_size([460.0, 320.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} messages", model.messages.len()));
                clear = ui
                    .add_enabled(!model.messages.is_empty(), egui::Button::new("Clear"))
                    .clicked();
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for e in model.messages.entries().rev() {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(&e.time);
                        match color(e.severity) {
                            Some(c) => ui.colored_label(c, e.severity.label()),
                            None => ui.label(e.severity.label()),
                        };
                        ui.label(&e.text);
                    });
                }
            });
        });
    if clear {
        model.messages.clear();
    }
    if !open {
        model.messages.set_open(false);
    }
}

/// The Messages button's text: "Messages", plus the unread count.
pub fn button_text(model: &AppModel) -> String {
    match model.messages.unread() {
        0 => "Messages".to_string(),
        n => format!("Messages ({n})"),
    }
}

/// The header's Messages button, `size` big (Note BH: as wide as Save, stacked on it).
pub fn button(ui: &mut egui::Ui, model: &mut AppModel, size: egui::Vec2) {
    if ui
        .add_sized(size, egui::Button::new(button_text(model)))
        .on_hover_text(format!(
            "Notes and results of this session ({})",
            super::actions::keys_text("messages")
        ))
        .clicked()
    {
        model.messages.toggle();
    }
}
