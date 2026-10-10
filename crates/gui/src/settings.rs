//! GUI-only settings (M3a spec §5.5): `%APPDATA%\sdc\gui.toml`. Never part of `sdc.toml`.
use encoders_sys::Bc1Model;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Preview size limits offered (M3a spec §6.5); 16384 only when `allow_16k_preview` is set.
pub const PREVIEW_SIZES: [u32; 5] = [1024, 2048, 4096, 8192, 16384];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bc1Choice {
    #[default]
    Ideal,
    IdealRound4,
    Nvidia,
    Amd,
}

impl Bc1Choice {
    pub const ALL: [Bc1Choice; 4] = [
        Bc1Choice::Ideal,
        Bc1Choice::IdealRound4,
        Bc1Choice::Nvidia,
        Bc1Choice::Amd,
    ];
    pub fn model(self) -> Bc1Model {
        match self {
            Bc1Choice::Ideal => Bc1Model::Ideal,
            Bc1Choice::IdealRound4 => Bc1Model::IdealRound4,
            Bc1Choice::Nvidia => Bc1Model::Nvidia,
            Bc1Choice::Amd => Bc1Model::Amd,
        }
    }
    /// The `sdc convert --bc1-decoder` value.
    pub fn cli(self) -> &'static str {
        match self {
            Bc1Choice::Ideal => "ideal",
            Bc1Choice::IdealRound4 => "ideal-round4",
            Bc1Choice::Nvidia => "nvidia",
            Bc1Choice::Amd => "amd",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
    pub export_folder: Option<PathBuf>,
    /// Most recent first, at most 10.
    pub recent: Vec<PathBuf>,
    /// Preview size limit (M3a spec §6.5), 4096 by default.
    pub preview_max: u32,
    pub allow_16k_preview: bool,
    /// Live preview encoding; off on a fresh install (M3a spec §6.3).
    pub live_encode: bool,
    /// Read at launch; a change applies after a restart (spec §4.6).
    pub bc1_decoder: Bc1Choice,
    /// Worker threads; 0 = one per core.
    pub jobs: usize,
    /// Memory budget in MiB; 0 = half of physical memory.
    pub mem_budget_mb: u64,
    pub columns_collapsed: bool,
    pub advanced_open: bool,
}

impl Default for GuiSettings {
    fn default() -> Self {
        GuiSettings {
            export_folder: None,
            recent: vec![],
            preview_max: 4096,
            allow_16k_preview: false,
            live_encode: false,
            bc1_decoder: Bc1Choice::Ideal,
            jobs: 0,
            mem_budget_mb: 0,
            columns_collapsed: false,
            advanced_open: false,
        }
    }
}

/// `%APPDATA%\sdc\gui.toml`.
pub fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("gui.toml"))
}

impl GuiSettings {
    /// Defaults when the file is missing; defaults and the error text when it cannot be read or parsed.
    pub fn load(path: &Path) -> (GuiSettings, Option<String>) {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (GuiSettings::default(), None),
            Err(e) => (
                GuiSettings::default(),
                Some(format!("{}: {e}", path.display())),
            ),
            Ok(text) => match toml::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (
                    GuiSettings::default(),
                    Some(format!("{}: {e}", path.display())),
                ),
            },
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
    pub fn push_recent(&mut self, p: PathBuf) {
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }
}
