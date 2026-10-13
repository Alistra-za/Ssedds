//! Where the user's Community Shaders lives, and whether it changed (00-decision §6.4).
use crate::cs_settings::SettingsPaths;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    pub root: PathBuf,
    pub owner_mod: Option<String>,
    pub hash: String,
}

/// Hash every `*.hlsl` / `*.hlsli` under `root`.
///
/// This reads every shader file (hundreds, several MB), so call it off the UI
/// thread. A missing / non-directory root or any unreadable entry is an error,
/// never a silently partial hash.
pub fn fingerprint(root: &Path, owner_mod: Option<String>) -> std::io::Result<Fingerprint> {
    if !root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("shaders root is not a directory: {}", root.display()),
        ));
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let e = entry.map_err(std::io::Error::from)?;
        if e.file_type().is_file()
            && e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("hlsl") || x.eq_ignore_ascii_case("hlsli"))
        {
            files.push(e.into_path());
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        h.update(
            f.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
                .as_bytes(),
        );
        h.update([0]);
        h.update(std::fs::read(f)?);
    }
    Ok(Fingerprint {
        root: root.to_path_buf(),
        owner_mod,
        hash: hex::encode(h.finalize()),
    })
}

/// Highest-priority origin providing `Shaders/Lighting.hlsl`. On equal
/// priority the origin listed LAST wins (`Iterator::max_by_key` semantics).
pub fn find_in_mods(origins: &[(String, PathBuf, i32)]) -> Option<(String, PathBuf)> {
    origins
        .iter()
        .filter(|(_, root, _)| root.join("Shaders").join("Lighting.hlsl").is_file())
        .max_by_key(|(_, _, prio)| *prio)
        .map(|(n, r, _)| (n.clone(), r.clone()))
}

/// Highest-priority origin that has the file `rel` (`/`-separated, relative to the origin root), and that
/// file's full path. Same tuples and tie rule as [`find_in_mods`]: MO2's overwrite (`i32::MAX`) beats every
/// mod, and game Data (-1) loses to all of them.
pub fn find_file(origins: &[(String, PathBuf, i32)], rel: &str) -> Option<(String, PathBuf)> {
    origins
        .iter()
        .map(|(n, root, prio)| (n, root.join(rel), *prio))
        .filter(|(_, f, _)| f.is_file())
        .max_by_key(|(_, _, prio)| *prio)
        .map(|(n, f, _)| (n.clone(), f))
}

const SETTINGS_USER: &str = "SKSE/Plugins/CommunityShaders/SettingsUser.json";
const SETTINGS_DEFAULT: &str = "SKSE/Plugins/CommunityShaders/SettingsDefault.json";
const INFO_INI: &str = "ShaderCache/Info.ini";

/// Where the user's CS settings are (P1-C C12). Through the MO2 origins when there are any (overwrite wins);
/// otherwise in the Data folder, which is the parent of the `Shaders` root (folder target, "Choose folder…").
/// A file that is not there, or a missing `Shaders/Features` folder, is `None`.
pub fn settings_paths(
    origins: Option<&[(String, PathBuf, i32)]>,
    shaders_root: &Path,
) -> SettingsPaths {
    let find = |rel: &str| match origins.filter(|o| !o.is_empty()) {
        Some(o) => find_file(o, rel).map(|(_, f)| f),
        None => shaders_root
            .parent()
            .map(|data| data.join(rel))
            .filter(|f| f.is_file()),
    };
    SettingsPaths {
        user: find(SETTINGS_USER),
        default: find(SETTINGS_DEFAULT),
        info_ini: find(INFO_INI),
        features_dir: Some(shaders_root.join("Features")).filter(|d| d.is_dir()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Same,
    Updated,
    Moved { old: Fingerprint, new: Fingerprint },
    NotFound { old: Option<Fingerprint> },
}

fn norm(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// (volume serial, file index) of a directory, Windows only.
#[cfg(windows)]
fn file_id(p: &Path) -> Option<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    };
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(p)
        .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is valid for the lifetime of `f`; `info` is a valid out pointer.
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }.ok()?;
    Some((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(not(windows))]
fn file_id(_p: &Path) -> Option<(u32, u64)> {
    None
}

/// Same folder? File identity when both open (handles drive-letter / mount
/// aliases of one volume), else a case- and separator-insensitive path compare.
fn same_root(a: &Path, b: &Path) -> bool {
    match (file_id(a), file_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => norm(a) == norm(b),
    }
}

fn same_owner(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        _ => false,
    }
}

pub fn compare(saved: Option<&Fingerprint>, found: Option<Fingerprint>) -> Change {
    match (saved, found) {
        (_, None) => Change::NotFound {
            old: saved.cloned(),
        },
        (None, Some(new)) => Change::Moved {
            old: Fingerprint {
                root: PathBuf::new(),
                owner_mod: None,
                hash: String::new(),
            },
            new,
        },
        (Some(old), Some(new))
            if same_root(&old.root, &new.root) && same_owner(&old.owner_mod, &new.owner_mod) =>
        {
            if old.hash == new.hash {
                Change::Same
            } else {
                Change::Updated
            }
        }
        (Some(old), Some(new)) => Change::Moved {
            old: old.clone(),
            new,
        },
    }
}
