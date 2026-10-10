#![windows_subsystem = "windows"]
//! `sdc-gui.exe`.
use engine::session::Session;
use sdc_gui::model::{AppModel, Dialog};
use sdc_gui::{app::SdcApp, fonts, settings};
use std::path::PathBuf;
use std::sync::Arc;

/// `%APPDATA%\sdc\config.toml`, the CLI's user config.
fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("config.toml"))
}

fn build_model() -> AppModel {
    let path = settings::settings_path();
    let (s, err) = path
        .as_deref()
        .map(settings::GuiSettings::load)
        .unwrap_or_default();
    // The BC1 decoder model is read once at launch (spec §4.6).
    let mut model = match Session::new(s.bc1_decoder.model()) {
        Ok(session) => {
            let mut m = AppModel::new(s, path, Arc::new(session));
            m.user_config = user_config_path();
            m
        }
        Err(e) => {
            let mut m = AppModel::placeholder(s);
            m.dialog = Some(Dialog::Error(format!("cannot start the engine: {e}")));
            m
        }
    };
    if let Some(e) = err {
        model.banner = Some(format!("Settings were not read, defaults are used: {e}"));
    }
    model
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("sdc")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "sdc",
        options,
        Box::new(|cc| {
            // The app has no usable font without the system fonts: show why and exit.
            if let Err(e) = fonts::install_system_fonts(&cc.egui_ctx) {
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("sdc")
                    .set_description(e.as_str())
                    .show();
                return Err(e.into());
            }
            Ok(Box::new(SdcApp::new(build_model())))
        }),
    )
}
