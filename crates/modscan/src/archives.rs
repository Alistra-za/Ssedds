//! Active BSAs in game order (plan 02 §3; engine order tagged [Claim] in 00-decision §3.2).
use crate::mo2::{Instance, LooseOrigin};
use crate::qtini::Ini;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ActiveBsa {
    pub path: PathBuf,
    pub providing_mod: Option<String>,
    pub rank: i32,
}

const IMPLICIT: [&str; 5] = [
    "skyrim.esm",
    "update.esm",
    "dawnguard.esm",
    "hearthfires.esm",
    "dragonborn.esm",
];

fn find(loose: &[LooseOrigin], file: &str) -> Option<(PathBuf, String)> {
    let mut best: Option<&LooseOrigin> = None;
    for o in loose {
        if o.root.join(file).is_file() && best.is_none_or(|b| o.priority > b.priority) {
            best = Some(o);
        }
    }
    best.map(|o| (o.root.join(file), o.name.clone()))
}

fn ini_lists(inst: &Instance) -> Vec<String> {
    let prof = inst.profiles.join(&inst.profile);
    let local = std::fs::read_to_string(prof.join("settings.ini"))
        .map(|t| Ini::parse(&t).get("General", "LocalSettings").as_deref() == Some("true"))
        .unwrap_or(false);
    let dir = if local {
        prof
    } else {
        let docs = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default();
        docs.join("Documents")
            .join("My Games")
            .join("Skyrim Special Edition")
    };
    let mut out = vec![];
    for key in ["sResourceArchiveList", "sResourceArchiveList2"] {
        let mut val = None;
        for f in ["skyrim.ini", "skyrimcustom.ini"] {
            if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
                if let Some(v) = Ini::parse(&t).get("Archive", key) {
                    val = Some(v);
                }
            }
        }
        out.extend(
            val.unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    out
}

pub fn active_bsas(inst: &Instance, loose: &[LooseOrigin]) -> Vec<ActiveBsa> {
    let mut out: Vec<ActiveBsa> = vec![];
    let push = |file: &str, out: &mut Vec<ActiveBsa>| {
        if out.iter().any(|b| {
            b.path
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(file))
        }) {
            return;
        }
        if let Some((path, m)) = find(loose, file) {
            let rank = out.len() as i32;
            out.push(ActiveBsa {
                path,
                providing_mod: Some(m).filter(|m| m != "Data"),
                rank,
            });
        }
    };
    for f in ini_lists(inst) {
        push(&f, &mut out);
    }
    let plugins = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("plugins.txt"))
        .unwrap_or_default();
    let active = IMPLICIT.iter().map(|s| s.to_string()).chain(
        plugins
            .lines()
            .filter_map(|l| l.strip_prefix('*'))
            .map(|s| s.trim().to_ascii_lowercase()),
    );
    for p in active {
        let stem = p.rsplit_once('.').map_or(p.as_str(), |x| x.0).to_string();
        push(&format!("{stem}.bsa"), &mut out);
        push(&format!("{stem} - textures.bsa"), &mut out);
    }
    out
}
