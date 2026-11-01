//! `docs/img/gui/shots.toml`: render settings, the shots and the exempt surfaces.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct Manifest {
    pub render: Render,
    #[serde(default)]
    pub shot: Vec<Shot>,
    #[serde(default)]
    pub exempt: Vec<Exempt>,
}

#[derive(Deserialize)]
pub struct Render {
    pub size: [u32; 2],
    pub pixels_per_point: f32,
    pub theme: String,
    pub adapter: String,
    /// Font file name -> sha256 of `%WINDIR%\Fonts\<file>`.
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct Shot {
    pub id: String,
    pub page: String,
    pub scenario: String,
    pub used_by: Vec<String>,
}

#[derive(Deserialize)]
pub struct Exempt {
    pub what: String,
    pub page: String,
    pub reason: String,
}

pub fn load(path: &Path) -> Manifest {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

impl Shot {
    pub fn png(&self, repo: &Path) -> PathBuf {
        repo.join("docs/img/gui")
            .join(&self.page)
            .join(format!("{}.png", self.id))
    }
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
