#![windows_subsystem = "windows"]
//! `sdc-gui.exe`.
use sdc_gui::{app::SdcApp, fonts, model::AppModel, settings};

fn main() -> eframe::Result {
    let path = settings::settings_path();
    let (s, _err) = path
        .as_deref()
        .map(settings::GuiSettings::load)
        .unwrap_or_default();
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
            Ok(Box::new(SdcApp {
                model: AppModel::placeholder(s),
            }))
        }),
    )
}
