//! Canonical form (00-decision §2 rule 5): ASCII-lowercase, '/' separators, relative to Data, starting
//! `textures/` — equal to `engine::discover::game_path` for the same file.
use crate::text::lossy;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathFlags {
    pub empty: bool,
    /// An absolute or `data/`-prefixed path was cut back to `textures/...`.
    pub rebased: bool,
    /// `textures/` was missing and was added.
    pub prefixed: bool,
    pub non_dds: bool,
}

#[derive(Debug, Clone, PartialEq)]
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
