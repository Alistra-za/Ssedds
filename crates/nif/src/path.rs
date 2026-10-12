//! Canonical form (00-decision §2 rule 5): ASCII-lowercase, '/' separators, relative to Data, starting
//! `textures/` — equal to `engine::discover::game_path` for the same file.
use crate::text::lossy;

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PathFlags {
    pub empty: bool,
    /// An absolute or `data/`-prefixed path was cut back to `textures/...`.
    pub rebased: bool,
    /// `textures/` was missing and was added.
    pub prefixed: bool,
    pub non_dds: bool,
}

impl PathFlags {
    /// Packed form for storage: 1 empty, 2 rebased, 4 prefixed, 8 non_dds.
    pub fn bits(&self) -> u8 {
        self.empty as u8
            | (self.rebased as u8) << 1
            | (self.prefixed as u8) << 2
            | (self.non_dds as u8) << 3
    }
    pub fn from_bits(b: u8) -> PathFlags {
        PathFlags {
            empty: b & 1 != 0,
            rebased: b & 2 != 0,
            prefixed: b & 4 != 0,
            non_dds: b & 8 != 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TexPath {
    pub raw: String,
    pub norm: String,
    pub flags: PathFlags,
}

fn slashes_lower(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .to_ascii_lowercase()
        .trim_start_matches('/')
        .to_string()
}

pub fn canon_texture(raw: &[u8]) -> TexPath {
    let raw_s = lossy(raw);
    let mut flags = PathFlags::default();
    let mut s = slashes_lower(&raw_s);
    if s.is_empty() {
        flags.empty = true;
        return TexPath {
            raw: raw_s,
            norm: s,
            flags,
        };
    }
    if !s.starts_with("textures/") {
        let comps: Vec<&str> = s.split('/').collect();
        if let Some(i) = comps.iter().rposition(|c| *c == "textures") {
            s = comps[i..].join("/");
            flags.rebased = true;
        } else {
            s = format!("textures/{s}");
            flags.prefixed = true;
        }
    }
    flags.non_dds = !s.ends_with(".dds");
    TexPath {
        raw: raw_s,
        norm: s,
        flags,
    }
}

pub fn canon_mesh(rel: &str) -> String {
    slashes_lower(rel)
}

pub fn is_facegen(norm: &str) -> bool {
    norm.starts_with("meshes/actors/character/facegendata/")
        || norm.starts_with("textures/actors/character/facegendata/")
}
