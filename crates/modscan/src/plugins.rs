//! The plugin pass (esp scanner spec §2.2): active plugins found through the MO2 origins, load-order FormIDs.
use crate::mo2::LooseOrigin;
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level `.esm/.esp/.esl` files of every loose origin, by lowercase name → (path, file name as on disk); the
/// highest-priority origin wins (overwrite > mods by priority > game Data). One `read_dir` per origin.
pub fn plugin_files(loose: &[LooseOrigin]) -> HashMap<String, (PathBuf, String)> {
    let mut best: HashMap<String, (PathBuf, String, i32)> = HashMap::new();
    for o in loose {
        let Ok(rd) = std::fs::read_dir(&o.root) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let low = name.to_ascii_lowercase();
            let plugin = [".esm", ".esp", ".esl"].iter().any(|x| low.ends_with(x));
            if !plugin || !e.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if best.get(&low).is_none_or(|b| o.priority > b.2) {
                best.insert(low, (e.path(), name, o.priority));
            }
        }
    }
    best.into_iter().map(|(k, (p, n, _))| (k, (p, n))).collect()
}

/// (base, mask) of each plugin's own FormIDs, in load order (ruling R2): full `LL << 24` / 0xFFFFFF with LL = the
/// count of earlier full plugins; light `0xFE000000 | LLL << 12` / 0xFFF with LLL = the count of earlier light ones.
/// `None` past the game's limits (254 full, 4096 light).
pub fn prefixes(light: &[bool]) -> Vec<Option<(u32, u32)>> {
    let (mut full, mut lite) = (0u32, 0u32);
    light
        .iter()
        .map(|&l| {
            if l {
                let i = lite;
                lite += 1;
                (i < 0x1000).then_some((0xFE00_0000 | (i << 12), 0xFFF))
            } else {
                let i = full;
                full += 1;
                (i < 0xFE).then_some((i << 24, 0x00FF_FFFF))
            }
        })
        .collect()
}

/// The local FormID with a master index past the master list read as the plugin itself, as the game reads it.
pub fn clamp_local(local: u32, n_masters: usize) -> u32 {
    let idx = (local >> 24) as usize;
    if idx > n_masters {
        ((n_masters.min(0xFF) as u32) << 24) | (local & 0x00FF_FFFF)
    } else {
        local
    }
}

/// A light plugin: the ESL header flag, or the `.esl` extension.
pub fn is_light(name: &str, flags: u32) -> bool {
    flags & esp::FLAG_ESL != 0 || name.to_ascii_lowercase().ends_with(".esl")
}
