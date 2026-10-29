//! `sdc-gui` (M3a spec; the program is Tessalith, `tessalith.exe`): a converter GUI over the engine. `model` holds all state and logic and has no egui types;
//! `ui` only draws it; `render` owns the GPU preview; `worker` runs engine calls off the UI thread.
/// The product name the user sees (window titles, dialogs). The package, CLI and on-disk names stay `sdc` (DECISIONS AG1).
pub const PRODUCT_NAME: &str = "Tessalith";

pub mod app;
pub mod crash_log;
pub mod fonts;
pub mod layout;
pub mod model;
pub mod pgp;
pub mod project;
pub mod render;
pub mod sets_doc;
pub mod settings;
pub mod ui;
pub mod worker;

/// Turns on the analysis disk cache (DECISIONS Note DZ): `<local_app_data>\sdc\analysis-cache`. `None` (no
/// `LOCALAPPDATA`) means no store. Only `main` reads the variable and passes it in.
pub fn with_analysis_cache(
    session: engine::session::Session,
    local_app_data: Option<&std::path::Path>,
) -> engine::session::Session {
    match local_app_data {
        Some(base) => session.with_disk_cache(base.join("sdc").join("analysis-cache")),
        None => session,
    }
}
