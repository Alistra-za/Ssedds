//! `sdc-gui` (M3a spec): a converter GUI over the engine. `model` holds all state and logic and has no egui types;
//! `ui` only draws it; `render` owns the GPU preview; `worker` runs engine calls off the UI thread.
pub mod app;
pub mod crash_log;
pub mod fonts;
pub mod model;
pub mod pgp;
pub mod project;
pub mod render;
pub mod sets_doc;
pub mod settings;
pub mod ui;
pub mod worker;
