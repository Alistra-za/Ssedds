//! A set's (or the defaults') JSON settings, by schema key path (`glint.screen_space_scale`). Colours are 0–255
//! integer arrays here; the writer converts them (DECISIONS P10).
pub type Settings = std::collections::BTreeMap<String, serde_json::Value>;
