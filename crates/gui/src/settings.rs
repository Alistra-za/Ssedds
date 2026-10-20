//! GUI-only settings (M3a spec §5.5): `%APPDATA%\sdc\gui.toml`. Never part of `sdc.toml`.
use encoders_sys::Bc1Model;
use serde::{Deserialize, Serialize};
mod layout;
pub use layout::*;

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
    /// The choice that selects `model`.
    pub fn from_model(model: Bc1Model) -> Bc1Choice {
        match model {
            Bc1Model::Ideal => Bc1Choice::Ideal,
            Bc1Model::IdealRound4 => Bc1Choice::IdealRound4,
            Bc1Model::Nvidia => Bc1Choice::Nvidia,
            Bc1Model::Amd => Bc1Choice::Amd,
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
    /// The Textures and Sets columns: shown at all (the header's checkboxes), and collapsed to a strip (the
    /// arrows on each column).
    pub textures_hidden: bool,
    pub sets_hidden: bool,
    pub textures_collapsed: bool,
    pub sets_collapsed: bool,
    /// A set shows its 3D preview instead of its slot cards (the set editor's 2D | 3D switch).
    pub set_view_3d: bool,
    /// The 3D view's settings window (TruePBR settings…) is open (AG132).
    pub set_settings_open: bool,
    pub advanced_open: bool,
    /// Reduce motion (Note CF1): shorter fades, no sliding or scaling.
    pub reduce_motion: bool,
    /// M5 (DECISIONS Note NV2): an explicit `nvcompress.exe`; `None` looks on PATH, then in the NVIDIA install folder.
    pub nvtt_path: Option<PathBuf>,
    /// The Meshes view's source (plan A Task 12), `[meshes]` in the file.
    pub meshes: MeshesSettings,
    /// The Meshes view's 3D panel (plan B Task B8), `[render3d]` in the file.
    pub render3d: Render3dSettings,
    /// The window and arrangement the app comes back to, `[layout]` in the file (DECISIONS AG3).
    #[serde(deserialize_with = "layout::lenient")]
    pub layout: LayoutSettings,
}

/// `[render3d]`: the Community Shaders `Shaders` folder last used, re-checked on start and on focus.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Render3dSettings {
    pub shaders: Option<render3d::locate::Fingerprint>,
    /// The set preview's encode cap in pixels (D5); 0 = 2048. Edited in `gui.toml` for now.
    pub set_preview_max: u32,
}

/// `[meshes]`: the MO2 instance and profile the Meshes view scans, or the folder chosen instead.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshesSettings {
    pub instance: Option<PathBuf>,
    pub profile: Option<String>,
    pub folder: Option<PathBuf>,
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
            textures_hidden: false,
            sets_hidden: false,
            textures_collapsed: false,
            sets_collapsed: false,
            set_view_3d: false,
            set_settings_open: false,
            advanced_open: false,
            reduce_motion: false,
            nvtt_path: None,
            meshes: MeshesSettings::default(),
            render3d: Render3dSettings::default(),
            layout: LayoutSettings::default(),
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
            Ok(text) => match toml::from_str::<GuiSettings>(&text) {
                Ok(mut s) => {
                    // The settings panel offers only these sizes; a hand-edited value becomes the default.
                    if !PREVIEW_SIZES.contains(&s.preview_max) {
                        s.preview_max = GuiSettings::default().preview_max;
                    }
                    s.layout.sanitize();
                    (s, None)
                }
                Err(e) => (
                    GuiSettings::default(),
                    Some(format!("{}: {e}", path.display())),
                ),
            },
        }
    }
    /// Write the file atomically (a temporary file, then rename). A file that exists but cannot be parsed (the user's
    /// hand edit that `load` reported) is kept as `gui.toml.bad` first, so the first save does not destroy it.
    /// Returns the `.bad` path when a broken file was kept.
    pub fn save(&self, path: &Path) -> std::io::Result<Option<PathBuf>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // #45: anything `load` could not read (not UTF-8, unparsable) is kept, not only a parse error.
        let unreadable = path.exists() && GuiSettings::load(path).1.is_some();
        let kept = path.with_extension("toml.bad");
        if unreadable {
            // Review M3: `rename` replaces an earlier `.bad` only once the move succeeds; a locked file keeps it.
            std::fs::rename(path, &kept)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        engine::dds::write_atomic(path, text.as_bytes())?;
        Ok(unreadable.then_some(kept))
    }

    /// [`Self::save`] that keeps hand edits made while the app runs (audit 2026-10-15 item 15). `base` is what was
    /// last loaded or written and the file's stamp then; when the file changed since, it is read again and every
    /// field the app has not changed since `base` takes the file's value. A file that is gone or does not parse is
    /// not merged. `base` is updated after a write.
    pub fn save_merged(
        &mut self,
        path: &Path,
        base: &mut Option<(GuiSettings, Option<engine::policy::SourceStamp>)>,
    ) -> std::io::Result<Option<PathBuf>> {
        let now = engine::policy::SourceStamp::of(path);
        if let Some((b, _)) = base.as_ref().filter(|(_, s)| now.is_some() && *s != now) {
            if let (disk, None) = GuiSettings::load(path) {
                if let Some(merged) = merge(b, self, &disk) {
                    *self = merged;
                }
            }
        }
        let kept = self.save(path)?;
        *base = Some((self.clone(), engine::policy::SourceStamp::of(path)));
        Ok(kept)
    }
    pub fn push_recent(&mut self, p: PathBuf) {
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }
}

/// Field by field (tables recursed): `mine` where it differs from `base`, else `disk`. `None` when a side does not
/// serialise or the result does not deserialise.
fn merge(base: &GuiSettings, mine: &GuiSettings, disk: &GuiSettings) -> Option<GuiSettings> {
    fn table(t: &toml::Table, base: &toml::Table, disk: &toml::Table) -> toml::Table {
        let mut out = disk.clone();
        let keys: std::collections::BTreeSet<&String> = t.keys().chain(base.keys()).collect();
        for k in keys {
            match (t.get(k), base.get(k), disk.get(k)) {
                (a, b, _) if a == b => {}
                // Recent folders merge per folder: the file's removals and additions, the app's additions and order.
                (Some(toml::Value::Array(a)), Some(toml::Value::Array(b)), d) if k == "recent" => {
                    let none = vec![];
                    let d = d.and_then(toml::Value::as_array).unwrap_or(&none);
                    let mut list: Vec<toml::Value> = a
                        .iter()
                        .filter(|x| d.contains(x) || !b.contains(x))
                        .cloned()
                        .collect();
                    for x in d {
                        if !b.contains(x) && !list.contains(x) {
                            list.push(x.clone());
                        }
                    }
                    out.insert(k.clone(), toml::Value::Array(list));
                }
                (
                    Some(toml::Value::Table(a)),
                    Some(toml::Value::Table(b)),
                    Some(toml::Value::Table(d)),
                ) => {
                    out.insert(k.clone(), toml::Value::Table(table(a, b, d)));
                }
                (Some(a), _, _) => {
                    out.insert(k.clone(), a.clone());
                }
                (None, _, _) => {
                    out.remove(k);
                }
            }
        }
        out
    }
    let v = |s: &GuiSettings| toml::Table::try_from(s).ok();
    table(&v(mine)?, &v(base)?, &v(disk)?).try_into().ok()
}
