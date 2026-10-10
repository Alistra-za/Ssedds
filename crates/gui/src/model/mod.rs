//! The view-model (M3a spec §3). Task 11 fills it in.
use crate::settings::GuiSettings;
use std::path::PathBuf;

pub struct AppModel {
    pub settings: GuiSettings,
    pub root: Option<PathBuf>,
}

impl AppModel {
    /// A model with no session, for drawing tests.
    pub fn placeholder(settings: GuiSettings) -> AppModel {
        AppModel {
            settings,
            root: None,
        }
    }
}
