//! PGPatcher (ParallaxGen) output as a derived layer (plan 05 §5). Detection by marker files, staleness by CRC32.
use nif::path::canon_mesh;
use std::collections::HashMap;
use std::path::Path;

pub fn running(names: &[String]) -> bool {
    names
        .iter()
        .any(|n| n == "pgpatcher.exe" || n == "pgtools.exe")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    None,
    Incomplete,
    Medium,
    Strong,
}

fn is_pg_esp(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "pgpatcher.esp"
        || n.strip_prefix("pg_")
            .and_then(|r| r.strip_suffix(".esp"))
            .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

pub fn signature(root: &Path) -> Signature {
    let names: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let diff = names
        .iter()
        .any(|n| n.eq_ignore_ascii_case("ParallaxGen_Diff.json"));
    let esp = names.iter().any(|n| is_pg_esp(n));
    let any_esp = names
        .iter()
        .any(|n| n.to_ascii_lowercase().ends_with(".esp"));
    let cache = names.iter().any(|n| {
        n.to_ascii_lowercase()
            .starts_with("pgpatcher_updatecache.bin")
    });
    let dups = root.join("meshes").join("_pgpatcher_dups").is_dir();
    let meshes = root.join("meshes").is_dir();
    if diff && esp {
        Signature::Strong
    } else if meshes && !diff && (any_esp || cache) && !dups {
        Signature::Incomplete
    } else if diff || esp || dups {
        Signature::Medium
    } else {
        Signature::None
    }
}

#[derive(Debug, Default)]
pub struct Diff {
    pub crc_orig: HashMap<String, u32>,
    pub crc_patched: HashMap<String, u32>,
}

pub fn read_diff(root: &Path) -> Option<Diff> {
    let text = std::fs::read_to_string(root.join("ParallaxGen_Diff.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut d = Diff::default();
    for (k, e) in v.as_object()? {
        let key = canon_mesh(k);
        if let Some(c) = e.get("crc32original").and_then(|x| x.as_u64()) {
            d.crc_orig.insert(key.clone(), c as u32);
        }
        if let Some(c) = e.get("crc32patched").and_then(|x| x.as_u64()) {
            d.crc_patched.insert(key, c as u32);
        }
    }
    Some(d)
}

pub fn dup_source(norm: &str) -> Option<String> {
    let rest = norm.strip_prefix("meshes/_pgpatcher_dups/")?;
    let (_, tail) = rest.split_once('/')?;
    Some(format!("meshes/{tail}"))
}

pub fn is_derived_path(norm: &str, diff: Option<&Diff>) -> bool {
    dup_source(norm).is_some() || diff.is_some_and(|d| d.crc_orig.contains_key(norm))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveCheck {
    Match,
    StaleChanged,
    StaleReordered,
    Unverified,
    UnknownSource,
    DupOf,
}

pub fn check(diff_crc: Option<u32>, source_crc: Option<u32>, deeper_crcs: &[u32]) -> DeriveCheck {
    match (diff_crc, source_crc) {
        (None, _) => DeriveCheck::UnknownSource,
        (Some(_), None) => DeriveCheck::Unverified,
        (Some(d), Some(s)) if d == s => DeriveCheck::Match,
        (Some(d), Some(_)) if deeper_crcs.contains(&d) => DeriveCheck::StaleReordered,
        _ => DeriveCheck::StaleChanged,
    }
}
