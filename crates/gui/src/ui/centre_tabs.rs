//! Note FB7 (review C6, amendment F): Presets and the TruePBR JSON editor are tabs of the centre column, not floating
//! windows. Pop out shows a view in an OS window of its own (for a second monitor); Dock brings it back.
use super::{json_editor, presets};
use crate::model::{AppModel, CentreTab};
use eframe::egui;

/// The views that can be docked or popped out: (tab, title, popped-out window size, body).
type Body = fn(&mut egui::Ui, &mut AppModel);
const VIEWS: [(CentreTab, &str, [f32; 2], Body); 2] = [
    (
        CentreTab::Presets,
        presets::TITLE,
        [900.0, 760.0],
        presets::view,
    ),
    (
        CentreTab::Json,
        json_editor::WINDOW_TITLE,
        [980.0, 760.0],
        json_editor::view,
    ),
];

/// (open, popped out) of a view; `Selection` has neither.
fn flags(model: &mut AppModel, tab: CentreTab) -> Option<(&mut bool, &mut bool)> {
    match tab {
        CentreTab::Selection => None,
        CentreTab::Presets => Some((&mut model.presets_open, &mut model.presets_popped)),
        CentreTab::Json => Some((&mut model.json_editor_open, &mut model.json_editor_popped)),
    }
}

/// The tab strip over the centre while a view is docked there: Selection, then each docked view. A click brings
/// that tab to the front. Accessible names end in "tab" so they never collide with the views' headings (rule 8.1).
pub fn strip(ui: &mut egui::Ui, model: &mut AppModel) {
    let docked: Vec<(CentreTab, &str)> = VIEWS
        .iter()
        .filter(|(t, ..)| flags(model, *t).is_some_and(|(open, popped)| *open && !*popped))
        .map(|(t, title, ..)| (*t, *title))
        .collect();
    if docked.is_empty() {
        return;
    }
    let front = model.centre_front();
    ui.horizontal_wrapped(|ui| {
        for (tab, title) in std::iter::once((CentreTab::Selection, "Selection")).chain(docked) {
            let on = front == tab;
            let text = egui::RichText::new(title);
            let r = ui.add(
                egui::Button::selectable(on, if on { text.strong() } else { text })
                    .frame_when_inactive(true),
            );
            r.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    on,
                    format!("{title} tab"),
                )
            });
            if tab == CentreTab::Selection {
                r.clone().on_hover_text("The selected texture or set");
            }
            if r.clicked() {
                model.centre_tab = tab;
            }
        }
    });
    ui.separator();
}

/// Close, then Pop out (docked) or Dock (popped out): for the right-to-left end of a view's heading row.
pub fn controls(ui: &mut egui::Ui, model: &mut AppModel, tab: CentreTab) {
    let Some((open, popped)) = flags(model, tab) else {
        return;
    };
    if ui.button("Close").clicked() {
        *open = false;
    }
    if *popped {
        if ui
            .button("Dock")
            .on_hover_text("Back into the centre column")
            .clicked()
        {
            *popped = false;
            model.centre_tab = tab;
        }
    } else if ui
        .button("Pop out")
        .on_hover_text("Show it in a window of its own, for a second monitor")
        .clicked()
    {
        *popped = true;
    }
}

/// Each popped-out view in its own OS window (an embedded one where the platform has none, and in tests). Closing the
/// window closes the view. Immediate viewports, as the JSON editor's window was: they draw with the app's model.
pub fn popped(ctx: &egui::Context, model: &mut AppModel) {
    for (tab, title, size, body) in VIEWS {
        if !flags(model, tab).is_some_and(|(open, popped)| *open && *popped) {
            continue;
        }
        let id = egui::ViewportId::from_hash_of(("sdc-centre-view", title));
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{title} — {}", crate::PRODUCT_NAME))
            .with_inner_size(size);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                if let Some((open, _)) = flags(model, tab) {
                    *open = false;
                }
            }
            match class {
                egui::ViewportClass::EmbeddedWindow => body(ui, model),
                _ => {
                    egui::CentralPanel::default().show(ui, |ui| body(ui, model));
                }
            }
        });
    }
}
