//! MO2 instance discovery and the loose-file origin order (plan 02 §1–§2).
use crate::qtini::Ini;
use crate::ScanError;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Instance {
    pub root: PathBuf,
    pub game_data: PathBuf,
    pub mods: PathBuf,
    pub overwrite: PathBuf,
    pub profiles: PathBuf,
    pub profile: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginKind {
    GameData,
    Mod,
    Overwrite,
}

#[derive(Debug, Clone)]
pub struct LooseOrigin {
    pub kind: OriginKind,
    pub name: String,
    pub root: PathBuf,
    pub priority: i32,
}

fn resolve(base: &Path, v: Option<String>, default: &str) -> PathBuf {
    let v = v.unwrap_or_else(|| format!("%BASE_DIR%/{default}"));
    let v = v.replace("%BASE_DIR%", &base.to_string_lossy());
    let p = PathBuf::from(v);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

impl Instance {
    pub fn load(root: &Path, profile: Option<&str>) -> Result<Instance, ScanError> {
        let text = std::fs::read_to_string(root.join("ModOrganizer.ini"))
            .map_err(|e| ScanError::NoInstance(format!("{}: {e}", root.display())))?;
        let ini = Ini::parse(&text);
        // A relative base_directory is relative to the instance root, never to the working directory.
        let base = ini
            .get("Settings", "base_directory")
            .map(|b| root.join(b))
            .unwrap_or_else(|| root.to_path_buf());
        let game = ini
            .get("General", "gamePath")
            .ok_or_else(|| ScanError::Ini("gamePath missing".into()))?;
        let profiles = resolve(&base, ini.get("Settings", "profiles_directory"), "profiles");
        let profile = match profile {
            Some(p) => p.to_string(),
            None => ini
                .get("General", "selected_profile")
                .ok_or_else(|| ScanError::Ini("selected_profile missing".into()))?,
        };
        if !profiles.join(&profile).join("modlist.txt").is_file() {
            return Err(ScanError::NoInstance(format!(
                "profile '{profile}' has no modlist.txt"
            )));
        }
        Ok(Instance {
            root: root.to_path_buf(),
            game_data: PathBuf::from(game).join("Data"),
            mods: resolve(&base, ini.get("Settings", "mod_directory"), "mods"),
            overwrite: resolve(
                &base,
                ini.get("Settings", "overwrite_directory"),
                "overwrite",
            ),
            profiles,
            profile,
        })
    }
}

/// Origins lowest → highest priority. modlist.txt is stored highest-first (plan 02 §2.1).
pub fn loose_origins(inst: &Instance) -> Result<(Vec<LooseOrigin>, Vec<String>), ScanError> {
    let text = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("modlist.txt"))
        .map_err(ScanError::Io)?;
    let mut enabled = vec![];
    let mut missing = vec![];
    for line in text.lines() {
        let Some(name) = line.strip_prefix('+') else {
            continue; // '-' disabled, '*' unmanaged, '#' comment
        };
        let name = name.trim();
        if name.ends_with("_separator") {
            continue;
        }
        let root = inst.mods.join(name);
        if root.is_dir() {
            enabled.push((name.to_string(), root));
        } else {
            missing.push(name.to_string());
        }
    }
    let mut out = vec![LooseOrigin {
        kind: OriginKind::GameData,
        name: "Data".into(),
        root: inst.game_data.clone(),
        priority: -1,
    }];
    for (i, (name, root)) in enabled.into_iter().rev().enumerate() {
        out.push(LooseOrigin {
            kind: OriginKind::Mod,
            name,
            root,
            priority: i as i32,
        });
    }
    out.push(LooseOrigin {
        kind: OriginKind::Overwrite,
        name: "overwrite".into(),
        root: inst.overwrite.clone(),
        priority: i32::MAX,
    });
    Ok((out, missing))
}

/// MO2 launch detection (plan 02 §1.2): usvfs loaded in this process, or parent process is ModOrganizer.exe.
#[cfg(windows)]
pub fn detect_launch() -> Option<PathBuf> {
    crate::win::mo2_exe_dir()
}

/// portable.txt next to ModOrganizer.exe → that folder; else the registry CurrentInstance (plan 02 §1.2).
#[cfg(windows)]
pub fn find_instance(exe_dir: &Path) -> Option<PathBuf> {
    if exe_dir.join("portable.txt").is_file() {
        return Some(exe_dir.to_path_buf());
    }
    let name = crate::win::current_instance_name()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("ModOrganizer").join(name))
}
