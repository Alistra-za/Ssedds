//! Active BSAs in game order (plan 02 §3; engine order tagged [Claim] in 00-decision §3.2).
//!
//! Order (lowest rank first): the vanilla `Skyrim - *.bsa` in the game Data folder (always active, owner rule),
//! then archives named by `sResourceArchiveList`/`sResourceArchiveList2`, then plugin-attached archives in
//! plugin order: the five base masters, `Skyrim.ccc` entries not listed in plugins.txt, then plugins.txt.
use crate::mo2::{Instance, LooseOrigin, OriginKind};
use crate::qtini::Ini;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// One plugin of the active load order (lowercase file name). `optional`: a `Skyrim.ccc` entry, which the game loads
/// only when the file exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePlugin {
    pub name: String,
    pub optional: bool,
}

/// The active load order (esp spec §2.2): the five base masters, `Skyrim.ccc` entries absent from plugins.txt, then
/// plugins.txt `*` entries; each name once, at its first place.
pub fn active_plugins(inst: &Instance) -> Vec<ActivePlugin> {
    let game = game_dir(inst);
    let plugins =
        read_text(&inst.profiles.join(&inst.profile).join("plugins.txt")).unwrap_or_default();
    let listed: std::collections::HashSet<String> = plugins
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let ccc: Vec<String> = read_text(&game.join("Skyrim.ccc"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !listed.contains(l))
        .collect();
    let mut seen = std::collections::HashSet::new();
    IMPLICIT
        .iter()
        .map(|s| (s.to_string(), false))
        .chain(ccc.into_iter().map(|s| (s, true)))
        .chain(
            plugins
                .lines()
                .filter_map(|l| l.strip_prefix('*'))
                .map(|s| (s.trim().to_ascii_lowercase(), false)),
        )
        .filter(|(n, _)| !n.is_empty() && seen.insert(n.clone()))
        .map(|(name, optional)| ActivePlugin { name, optional })
        .collect()
}

/// Default SSE order of the vanilla archives (lowercase). Other `skyrim - *.bsa` follow alphabetically.
const VANILLA: [&str; 18] = [
    "skyrim - misc.bsa",
    "skyrim - shaders.bsa",
    "skyrim - interface.bsa",
    "skyrim - animations.bsa",
    "skyrim - meshes0.bsa",
    "skyrim - meshes1.bsa",
    "skyrim - sounds.bsa",
    "skyrim - voices_en0.bsa",
    "skyrim - textures0.bsa",
    "skyrim - textures1.bsa",
    "skyrim - textures2.bsa",
    "skyrim - textures3.bsa",
    "skyrim - textures4.bsa",
    "skyrim - textures5.bsa",
    "skyrim - textures6.bsa",
    "skyrim - textures7.bsa",
    "skyrim - textures8.bsa",
    "skyrim - patch.bsa",
];

/// Top-level `.bsa` files of every loose origin, by lowercase name → (on-disk path, origin name, priority of the
/// highest-priority origin providing it). One `read_dir` per origin.
struct BsaIndex {
    by_name: HashMap<String, (PathBuf, String, i32)>,
    /// Lowercase names of `.bsa` files in the game Data origin.
    data: Vec<String>,
}

impl BsaIndex {
    fn build(loose: &[LooseOrigin]) -> BsaIndex {
        let mut by_name: HashMap<String, (PathBuf, String, i32)> = HashMap::new();
        let mut data = vec![];
        for o in loose {
            let Ok(rd) = std::fs::read_dir(&o.root) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let low = name.to_ascii_lowercase();
                if !low.ends_with(".bsa") || !e.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                if o.kind == OriginKind::GameData {
                    data.push(low.clone());
                }
                let better = by_name.get(&low).is_none_or(|b| o.priority > b.2);
                if better {
                    by_name.insert(low, (e.path(), o.name.clone(), o.priority));
                }
            }
        }
        BsaIndex { by_name, data }
    }
}

fn read_ini(p: &Path) -> Option<Ini> {
    read_text(p).map(|t| Ini::parse(&t))
}

/// `None` when the file cannot be read; decoding is `crate::read_text`.
fn read_text(p: &Path) -> Option<String> {
    crate::read_text(p).ok()
}

/// Archive-list entries of `Skyrim.ini` + `SkyrimCustom.ini` in `dir` (later file wins per key); `None` when no
/// file there defines either key.
fn ini_lists_in(dir: &Path) -> Option<Vec<String>> {
    let inis: Vec<Ini> = ["skyrim.ini", "skyrimcustom.ini"]
        .iter()
        .filter_map(|f| read_ini(&dir.join(f)))
        .collect();
    lists_of(&inis)
}

fn lists_of(inis: &[Ini]) -> Option<Vec<String>> {
    let mut any = false;
    let mut out = vec![];
    for key in ["sResourceArchiveList", "sResourceArchiveList2"] {
        let mut val = None;
        for ini in inis {
            if let Some(v) = ini.get("Archive", key) {
                val = Some(v);
            }
        }
        any |= val.is_some();
        out.extend(
            val.unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    any.then_some(out)
}

fn game_dir(inst: &Instance) -> PathBuf {
    inst.game_data
        .parent()
        .map_or_else(|| inst.game_data.clone(), Path::to_path_buf)
}

/// The folder holding the game INIs for this profile: the profile itself with `LocalSettings=true`, else
/// `<Documents>\My Games\Skyrim Special Edition` (Documents from the shell, so a OneDrive-redirected folder works).
pub fn ini_dir(inst: &Instance) -> Option<PathBuf> {
    let prof = inst.profiles.join(&inst.profile);
    let local = read_ini(&prof.join("settings.ini"))
        .and_then(|i| i.get("General", "LocalSettings"))
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if local {
        return Some(prof);
    }
    documents_dir().map(|d| d.join("My Games").join("Skyrim Special Edition"))
}

#[cfg(windows)]
fn documents_dir() -> Option<PathBuf> {
    crate::win::documents_dir()
}

#[cfg(not(windows))]
fn documents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
}

/// Active BSAs with the INI folder of [`ini_dir`]. Returns the archives and informational notes.
pub fn active_bsas(inst: &Instance, loose: &[LooseOrigin]) -> (Vec<ActiveBsa>, Vec<String>) {
    active_bsas_with(inst, loose, ini_dir(inst).as_deref())
}

/// Active BSAs, reading the game INIs from `ini_dir` (`None`: no INI folder). If neither INI there defines an
/// archive list, `<game dir>\Skyrim_Default.ini` is used; if that has none either, a note says so (vanilla and
/// plugin archives are still ranked).
pub fn active_bsas_with(
    inst: &Instance,
    loose: &[LooseOrigin],
    ini_dir: Option<&Path>,
) -> (Vec<ActiveBsa>, Vec<String>) {
    let idx = BsaIndex::build(loose);
    let mut notes = vec![];
    let mut out: Vec<ActiveBsa> = vec![];
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut push = |file: &str, out: &mut Vec<ActiveBsa>| {
        let low = file.to_ascii_lowercase();
        if taken.contains(&low) {
            return;
        }
        if let Some((path, m, _)) = idx.by_name.get(&low) {
            taken.insert(low);
            let rank = out.len() as i32;
            out.push(ActiveBsa {
                path: path.clone(),
                providing_mod: Some(m.clone()).filter(|m| m != "Data"),
                rank,
            });
        }
    };

    // 1. Vanilla archives present in the game Data folder: always active, default SSE order, then the rest.
    let mut vanilla: Vec<&String> = idx
        .data
        .iter()
        .filter(|n| n.starts_with("skyrim - "))
        .collect();
    vanilla.sort_by_key(|n| {
        (
            VANILLA.iter().position(|v| v == n).unwrap_or(VANILLA.len()),
            *n,
        )
    });
    for n in vanilla {
        push(n, &mut out);
    }

    // 2. INI archive lists add further archives in their listed order.
    let game = game_dir(inst);
    let lists = ini_dir
        .and_then(ini_lists_in)
        .or_else(|| read_ini(&game.join("Skyrim_Default.ini")).and_then(|i| lists_of(&[i])));
    match lists {
        Some(l) => {
            for f in l {
                push(&f, &mut out);
            }
        }
        None => notes.push(format!(
            "no sResourceArchiveList in {} or Skyrim_Default.ini; only vanilla and plugin archives are ranked",
            ini_dir.map_or_else(|| "(no INI folder)".into(), |d| d.display().to_string())
        )),
    }

    // 3. Plugin-attached archives: base masters, Skyrim.ccc entries absent from plugins.txt, plugins.txt.
    for p in active_plugins(inst) {
        let stem = p
            .name
            .rsplit_once('.')
            .map_or(p.name.as_str(), |x| x.0)
            .to_string();
        push(&format!("{stem}.bsa"), &mut out);
        push(&format!("{stem} - textures.bsa"), &mut out);
    }
    (out, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trace-debug #24: a BOM must not hide the first section of Skyrim.ini.
    #[test]
    fn ini_lists_read_past_a_bom_and_an_ansi_byte() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("Skyrim.ini"),
            b"\xEF\xBB\xBF[Archive]\nsResourceArchiveList=A.bsa, B.bsa\n[Other]\nsName=Caf\xE9\n",
        )
        .unwrap();
        assert_eq!(ini_lists_in(d.path()).unwrap(), ["A.bsa", "B.bsa"]);
    }
}
