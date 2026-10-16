#![windows_subsystem = "windows"]
//! `tessalith.exe`.
use engine::session::Session;
use sdc_gui::model::{AppModel, Dialog};
use sdc_gui::{app::SdcApp, fonts, layout, render, settings};
use std::path::PathBuf;
use std::sync::Arc;

/// `%APPDATA%\sdc\config.toml`, the CLI's user config.
fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("config.toml"))
}

/// `%APPDATA%\sdc\presets`, the CLI's user presets folder (M6 Track D spec §3).
fn user_presets_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("presets"))
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
            m.user_presets = user_presets_path();
            m.scan_db = sdc_gui::model::meshes::default_store_path();
            m
        }
        Err(e) => {
            let mut m = AppModel::placeholder(s);
            m.dialog = Some(Dialog::Error(format!("cannot start the engine: {e}")));
            m
        }
    };
    model.refresh_nvtt();
    if let Some(e) = err {
        model.keep_banner(format!("Settings were not read, defaults are used: {e}"));
    }
    model
}

fn main() -> eframe::Result {
    sdc_gui::crash_log::install();
    // The preview timing log (SDC_PREVIEW_LOG=0 turns it off); kept alive for the whole run.
    let _perf = render3d::perf_log::start();
    // The saved window (AG17, AG18), fitted to the monitors before the window exists.
    let saved = settings::settings_path()
        .map(|p| settings::GuiSettings::load(&p).0.layout)
        .unwrap_or_default();
    let win = layout::initial_window(&saved, &layout::monitors());
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(sdc_gui::PRODUCT_NAME)
        .with_inner_size(win.size)
        .with_maximized(win.maximized);
    if let Some(p) = win.pos {
        viewport = viewport.with_position(p);
    }
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/tessalith-256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => eprintln!("window icon skipped: {e}"),
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        // BC texture compression and the adapter's own limits, for the preview textures.
        wgpu_options: render::wgpu_configuration(),
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        sdc_gui::PRODUCT_NAME,
        options,
        Box::new(|cc| {
            // The app has no usable font without the system fonts: show why and exit.
            if let Err(e) = fonts::install_system_fonts(&cc.egui_ctx) {
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title(sdc_gui::PRODUCT_NAME)
                    .set_description(e.as_str())
                    .show();
                return Err(e.into());
            }
            if let Some(rs) = &cc.wgpu_render_state {
                sdc_gui::crash_log::watch_device(rs, sdc_gui::crash_log::run_log_path());
            }
            // No render state means no usable adapter: the app still starts, without a preview.
            let app = SdcApp::new(build_model()).with_render_state(cc.wgpu_render_state.as_ref());
            app.restore_ctx(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
