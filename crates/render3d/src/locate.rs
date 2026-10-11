//! Where the user's Community Shaders lives, and whether it changed (00-decision §6.4).
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    pub root: PathBuf,
    pub owner_mod: Option<String>,
    pub hash: String,
}

pub fn fingerprint(root: &Path, owner_mod: Option<String>) -> std::io::Result<Fingerprint> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("hlsl") || x.eq_ignore_ascii_case("hlsli"))
        })
        .collect();
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        h.update(
            f.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
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

pub fn find_in_mods(origins: &[(String, PathBuf, i32)]) -> Option<(String, PathBuf)> {
    origins
        .iter()
        .filter(|(_, root, _)| root.join("Shaders").join("Lighting.hlsl").is_file())
        .max_by_key(|(_, _, prio)| *prio)
        .map(|(n, r, _)| (n.clone(), r.clone()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Same,
    Updated,
    Moved { old: Fingerprint, new: Fingerprint },
    NotFound { old: Option<Fingerprint> },
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
        (Some(old), Some(new)) if old.root == new.root && old.owner_mod == new.owner_mod => {
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
