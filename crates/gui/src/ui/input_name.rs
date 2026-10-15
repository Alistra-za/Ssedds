//! GUI tweaks G1: the set editor's input name picker. Drawing only; the change is `AppModel::set_input_name`.
use super::columns::RED;
use super::path_picker;
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use std::sync::Arc;

pub fn picker(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    let Some(current) = model.input_name_of(&v.id) else {
        return;
    };
    let why = model
        .project
        .as_ref()
        .and_then(|p| p.error())
        .map(|e| format!("sdc.toml is read-only: {e}"));
    let err_id = egui::Id::new(("input-name-error", &v.id));
    ui.add_enabled_ui(why.is_none(), |ui| {
        path_picker::menu(ui, "Input name…", |ui| {
            ui.label(egui::RichText::new("Read this set's textures as").strong());
            ui.weak(format!("now: {current}"));
            let key = (Arc::as_ptr(&model.known) as usize, model.items_rev());
            let paths = path_picker::cached(ui, "input-name", key, || model.known_textures(Some(0)));
            if let Some(p) = path_picker::list(ui, ("input-name", &v.id), &paths, true) {
                match model.set_input_name(&v.id, &p) {
                    Ok(()) => {
                        ui.data_mut(|d| d.remove::<String>(err_id));
                        ui.close();
                    }
                    Err(e) => {
                        ui.data_mut(|d| d.insert_temp(err_id, e));
                    }
                }
            }
            if let Some(e) = ui.data(|d| d.get_temp::<String>(err_id)) {
                ui.colored_label(RED, e);
            }
        })
        .response
        .on_hover_text(
            "Pick the game path this set's textures are read as. Each texture keeps its own suffix; files on \
             disk are not renamed (an [inputs] alias in sdc.toml).",
        );
    })
    .response
    .on_disabled_hover_text(why.unwrap_or_default());
}
