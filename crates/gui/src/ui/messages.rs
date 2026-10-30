//! Item 20 (DECISIONS AG112-AG114): the toasts at the bottom right and the Messages tab. The toasts are drawn once a
//! frame by `SdcApp` for both views; the store is `model::messages`.
use super::motion;
use super::status::{AMBER, RED};
use crate::model::messages::{Severity, Toast, ToastAction, FADE_OUT};
use crate::model::AppModel;
use eframe::egui::{self, emath};
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn color(s: Severity) -> Option<egui::Color32> {
    match s {
        Severity::Info => None,
        Severity::Warning => Some(AMBER),
        Severity::Error => Some(RED),
    }
}

/// The toasts of the last frame: id, rect (the pointer over one pauses its time), whether its entry has reached
/// full opacity once (the rise runs only until then), and whether it is exiting (only a hover reverses an exit).
#[derive(Clone, Default)]
struct Rects(Vec<(u64, egui::Rect, bool, bool)>);

/// Toast entry: the fade in and the rise, in seconds and points.
const ENTER: f32 = 0.20;
const RISE: f32 = 8.0;

/// The egui animation id of toast `toast_id`'s opacity.
#[doc(hidden)]
pub fn toast_anim_id(toast_id: u64) -> egui::Id {
    egui::Id::new(("toast", toast_id))
}

/// Advances and draws the toasts. The Messages log is the bottom panel's tab (`tab`).
pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    draw_at(ctx, model, Instant::now());
}

/// `draw`'s toast half at clock `now`. A toast's life runs on the model clock; its fades and rise run on egui time
/// (Note CF2, R6), so a hovered toast (whose clock stops) still finishes its entry.
pub fn draw_at(ctx: &egui::Context, model: &mut AppModel, now: Instant) {
    let id = egui::Id::new("toast-rects");
    let last: Rects = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let pointer = ctx.pointer_hover_pos();
    let hovered: HashSet<u64> = last
        .0
        .iter()
        .filter(|(_, r, ..)| pointer.is_some_and(|p| r.contains(p)))
        .map(|(i, ..)| *i)
        .collect();
    model.messages.tick(now, &hovered);
    let toasts: Vec<Toast> = model.messages.toasts().to_vec();
    let enter = motion::fade_dur(ctx, ENTER);
    let fade = Duration::from_secs_f32(motion::fade_dur(ctx, FADE_OUT.as_secs_f32()));
    let rise = if motion::reduce_motion(ctx) {
        0.0
    } else {
        RISE
    };
    let mut rects = Rects::default();
    let mut clicked: Option<(u64, ToastAction)> = None;
    if !toasts.is_empty() {
        // shortcut: the anchored Area places itself from last frame's size, so a new toast's first visible frame
        // draws the stack one toast low; `request_discard` would fix it but can handle a click twice. Upgrade if
        // egui sizes anchored Areas in the same pass.
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
            .order(egui::Order::Foreground)
            // Clicks pass through, unless a toast has a button (Note FB5).
            .interactable(toasts.iter().any(|t| t.action.is_some()))
            // Each toast fades itself; the Area's own fade would fade the first one twice.
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                for t in &toasts {
                    let key = toast_anim_id(t.id);
                    let prev = last.0.iter().find(|(i, ..)| *i == t.id);
                    // Sticky: reduce motion shrinking `fade` mid-exit must not brighten the toast again.
                    let exiting =
                        !hovered.contains(&t.id) && (t.left <= fade || prev.is_some_and(|p| p.3));
                    let target = !exiting;
                    let ease = emath::easing::quadratic_out;
                    if prev.is_none() {
                        // First-call rule: seed closed, so the first frame starts the entry.
                        ctx.animate_bool_with_time_and_easing(key, false, enter, ease);
                    }
                    let time = if target { enter } else { fade.as_secs_f32() };
                    let a = ctx.animate_bool_with_time_and_easing(key, target, time, ease);
                    let risen = prev.is_some_and(|p| p.2) || a >= 1.0;
                    let dy = if risen { 0.0 } else { (1.0 - a) * rise };
                    // The scope carries the opacity to the frame and its shadow, not only the text (01#1).
                    let r = ui.scope(|ui| {
                        ui.multiply_opacity(a);
                        let shift = emath::TSTransform::from_translation(egui::vec2(0.0, dy));
                        ui.with_visual_transform(shift, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                let text = |ui: &mut egui::Ui| match color(t.severity) {
                                    Some(c) => ui.colored_label(c, &t.text),
                                    None => ui.label(&t.text),
                                };
                                let Some(action) = t.action else {
                                    text(ui);
                                    return;
                                };
                                ui.horizontal(|ui| {
                                    text(ui);
                                    let b = ui.small_button("Undo");
                                    // The header has an Undo too: this one names what it takes back.
                                    let name = match action {
                                        ToastAction::UndoCrashRestore => {
                                            "Undo the restored crash edits"
                                        }
                                        ToastAction::UndoDeleteSet(..) => "Undo the deleted set",
                                    };
                                    b.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            name,
                                        )
                                    });
                                    if b.clicked() {
                                        clicked = Some((t.id, action));
                                    }
                                });
                            })
                        })
                    });
                    rects.0.push((t.id, r.response.rect, risen, exiting));
                }
            });
    }
    ctx.data_mut(|d| d.insert_temp(id, rects));
    if let Some((toast, action)) = clicked {
        model.messages.dismiss(toast);
        match action {
            ToastAction::UndoCrashRestore => model.undo_crash_restore(),
            ToastAction::UndoDeleteSet(depth, pushes) => model.undo_delete_set(depth, pushes),
        }
    }
    // A resting toast wakes once, when its fade is due; the animate call above repaints while anything moves. A
    // hovered toast's clock is stopped, so it needs no timer: the pointer leaving is an input event.
    let next = toasts
        .iter()
        .filter(|t| !hovered.contains(&t.id))
        .map(|t| if t.left > fade { t.left - fade } else { t.left })
        .min();
    if let Some(d) = next {
        ctx.request_repaint_after(d);
    }
}

/// The Messages tab: time, severity and text of the last entries, newest first, and a Clear button.
pub fn tab(ui: &mut egui::Ui, model: &mut AppModel) {
    ui.horizontal(|ui| {
        let n = model.messages.len();
        ui.label(format!("{n} message{}", super::review::plural(n)));
        if ui
            .add_enabled(!model.messages.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            model.messages.clear();
        }
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
}

/// The Messages tab's text: "Messages", plus the unread count.
pub fn button_text(model: &AppModel) -> String {
    match model.messages.unread() {
        0 => "Messages".to_string(),
        n => format!("Messages ({n})"),
    }
}
