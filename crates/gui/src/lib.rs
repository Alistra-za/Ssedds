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
