//! "Make marker cube…" dialog (cubemap GUI step, Task 6). The draft is `model.marker`; Save… opens the file dialog and
//! the model writes on its result (`model::marker`). Enter is Save…, which only opens a dialog; Esc and Cancel close.
use super::dialogs::{dialog_hover, Role};
use crate::model::picker::BUSY_HOVER;
use crate::model::AppModel;
use eframe::egui;

/// Help under the colour fields (the CLI's `--color` help).
pub const COLOUR_HELP: &str = "F0 = (code/255)^1.6";
/// The roughness checkbox's name.
pub const ROUGHNESS_ON: &str = "Set roughness";

fn code_field(ui: &mut egui::Ui, label: &str, v: &mut u8) {
    let l = ui.label(label);
    ui.add(egui::DragValue::new(v).range(0..=255).speed(1))
        .labelled_by(l.id);
}

pub fn draw(ctx: &egui::Context, model: &mut AppModel) {
    let busy = model.picker.busy();
    let mut d = model.marker.clone();
    let buttons = [
        ("Cancel", Role::Plain, true),
        ("Save…", Role::Default, !busy),
    ];
    let (k, clicked) = dialog_hover(
        ctx,
        "make-marker",
        false,
        420.0,
        "Make marker cube",
        &buttons,
        &[None, Some(BUSY_HOVER)],
        |ui| {
            ui.label(
                "A 1×1 cube that Community Shaders reads as a marker: the colour is the F0 reflectance, the alpha \
                 the roughness.",
            );
            ui.horizontal(|ui| {
                let r = ui.color_edit_button_srgb(&mut d.rgb);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Marker colour")
                });
                code_field(ui, "R", &mut d.rgb[0]);
                code_field(ui, "G", &mut d.rgb[1]);
                code_field(ui, "B", &mut d.rgb[2]);
            });
            ui.weak(COLOUR_HELP);
            let mut on = d.roughness.is_some();
            if ui.checkbox(&mut on, ROUGHNESS_ON).changed() {
                d.roughness = on.then_some(0.5);
            }
            if let Some(r) = d.roughness.as_mut() {
                ui.add(egui::Slider::new(r, 0.0..=1.0).text("Roughness"));
            }
            match engine::cube::marker::alpha_of(d.roughness) {
                Ok(a) => ui.weak(format!(
                    "Alpha {a}{}",
                    if d.roughness.is_none() {
                        " (default)"
                    } else {
                        ""
                    }
                )),
                Err(e) => ui.colored_label(super::status::RED, e),
            };
        },
    );
    model.marker = d;
    if clicked == Some(0) || k.cancel {
        model.dialog = None;
    } else if clicked == Some(1) || (k.enter && !busy) {
        model.pick_marker_file(ctx);
    }
}
