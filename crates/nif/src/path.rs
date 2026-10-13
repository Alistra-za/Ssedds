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

/// Cuts `s` back to its last `root` component (true), or prefixes `root/` when there is none (false).
fn rebase(s: &str, root: &str) -> (bool, String) {
    let comps: Vec<&str> = s.split('/').collect();
    match comps.iter().rposition(|c| *c == root) {
        Some(i) => (true, comps[i..].join("/")),
        None => (false, format!("{root}/{s}")),
    }
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
        let (rebased, t) = rebase(&s, "textures");
        s = t;
        flags.rebased = rebased;
        flags.prefixed = !rebased;
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

/// A plugin model path (`MODL`…, relative to `meshes\`) in canonical form: lower case, `/`, starting `meshes/`. An
/// absolute or `data\`-prefixed path is cut back to its last `meshes` component (esp scanner spec §2.1).
pub fn canon_model(raw: &[u8]) -> String {
    let s = slashes_lower(&lossy(raw));
    if s.is_empty() || s.starts_with("meshes/") {
        return s;
    }
    rebase(&s, "meshes").1
}

/// PGPatcher's LOD blocklist (`*\lod\*`, `*\dyndolod\*`, `*_lod_*`, `*_lod.*`) on a canonical lower-case path.
pub fn is_lod_path(norm: &str) -> bool {
    ["/lod/", "/dyndolod/", "_lod_", "_lod."]
        .iter()
        .any(|p| norm.contains(p))
}

pub fn is_facegen(norm: &str) -> bool {
    norm.starts_with("meshes/actors/character/facegendata/")
        || norm.starts_with("textures/actors/character/facegendata/")
}

#[cfg(test)]
mod lod_tests {
    use super::*;

    #[test]
    fn lod_paths_match_the_pgpatcher_blocklist_only() {
        for p in [
            "meshes/lod/x.nif",
            "meshes/dyndolod/x.nif",
            "meshes/a/b_lod_0.nif",
            "textures/t/x_lod.dds",
        ] {
            assert!(is_lod_path(p), "{p}");
        }
        assert!(is_lod_path(&canon_model(b"Meshes\\LOD\\X.NIF")));
        for p in [
            "meshes/clod/x.nif",
            "meshes/lodge/x.nif",
            "meshes/a/blodgett.nif",
            "textures/flood.dds",
        ] {
            assert!(!is_lod_path(p), "{p}");
        }
    }
}
