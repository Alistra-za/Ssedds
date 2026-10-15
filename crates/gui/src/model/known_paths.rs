//! GUI tweaks G1/G5: the texture and mesh paths the mod scan knows, for the path pickers (the set editor's input
//! name and the PGPatcher JSON editor's match values). Read from the scan database on a worker after a scan and at
//! start; empty without a scan.
use super::sets::{set_views, SlotFill};
use super::AppModel;
use crate::worker::{spawn_guarded, WorkerMsg};
use modscan::store::Store;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What the scan knows. Paths are canonical: lower case, `/`, starting `textures/` or `meshes/`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KnownPaths {
    /// Texture path (with `.dds`) → the lighting slots (0-based bits) any scanned mesh uses it in.
    pub textures: BTreeMap<String, u16>,
    /// Every folder that holds a winning mesh, and its parent folders, without `meshes/` (`architecture/whiterun`).
    pub mesh_dirs: Vec<String>,
}

/// Read the known paths from scan database `db`.
pub fn read(db: &Path) -> Result<KnownPaths, String> {
    let store = Store::open_read_only(db).map_err(|e| e.to_string())?;
    let conn = store.conn();
    let mut textures: BTreeMap<String, u16> = BTreeMap::new();
    let mut st = conn
        .prepare(
            "SELECT DISTINCT p.p, tu.slot FROM tex_use tu JOIN path p ON p.id = tu.tex_path_id \
             WHERE tu.slot < 16",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (p, slot) = row.map_err(|e| e.to_string())?;
        if p.starts_with("textures/") {
            *textures.entry(p).or_default() |= 1 << slot;
        }
    }
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let mut st = conn
        .prepare("SELECT p.p FROM nif_winner w JOIN path p ON p.id = w.path_id")
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let p = row.map_err(|e| e.to_string())?;
        let p = p.strip_prefix("meshes/").unwrap_or(&p);
        let mut cur = p;
        while let Some(i) = cur.rfind('/') {
            cur = &cur[..i];
            if !dirs.insert(cur.to_string()) {
                break;
            }
        }
    }
    Ok(KnownPaths {
        textures,
        mesh_dirs: dirs.into_iter().collect(),
    })
}

/// `p` without a `.dds` ending.
pub fn strip_dds(p: &str) -> &str {
    match p.len().checked_sub(4) {
        Some(c) if p.is_char_boundary(c) && p[c..].eq_ignore_ascii_case(".dds") => &p[..c],
        _ => p,
    }
}

/// The first `limit` of `list` that contain every word of `query` (case-insensitive, `\` read as `/`).
pub fn filter<'a>(list: &'a [String], query: &str, limit: usize) -> Vec<&'a str> {
    let q = query.trim().replace('\\', "/").to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    list.iter()
        .map(String::as_str)
        .filter(|p| {
            let l = p.to_lowercase();
            words.iter().all(|w| l.contains(w))
        })
        .take(limit)
        .collect()
}

impl AppModel {
    /// Read the known paths again from the scan database, on a worker (at start and after a scan).
    pub fn refresh_known_paths(&mut self) {
        let Some(db) = self.scan_db.clone().filter(|d| d.is_file()) else {
            return;
        };
        spawn_guarded(self.tx.clone(), "known-paths", move |tx| {
            let result = std::panic::catch_unwind(|| read(&db))
                .unwrap_or_else(|p| Err(format!("panic: {}", crate::worker::panic_text(&*p))));
            let _ = tx.send(WorkerMsg::KnownPaths(result));
        });
    }

    pub(super) fn known_paths_landed(&mut self, result: Result<KnownPaths, String>) {
        match result {
            Ok(k) => self.set_known(k),
            Err(e) => eprintln!("tessalith: reading the scan's texture paths failed: {e}"),
        }
    }

    /// Texture paths for a picker, without `.dds`: the scan's paths used in lighting slot `slot` (0-based; `None`
    /// for any slot), then this project's own outputs' game paths. Sorted, no duplicates.
    pub fn known_textures(&self, slot: Option<u8>) -> Vec<String> {
        let mut out: BTreeSet<String> = self
            .known
            .textures
            .iter()
            .filter(|(_, bits)| slot.is_none_or(|s| *bits & (1 << s) != 0))
            .map(|(p, _)| strip_dds(p).to_string())
            .collect();
        for v in set_views(self) {
            for (kind, fill) in &v.slots {
                if slot.is_some_and(|s| s + 1 != kind.slot_number()) {
                    continue;
                }
                if let SlotFill::Own { item } = fill {
                    if let Some(j) = self.item(item).and_then(|i| i.job.as_ref()) {
                        if j.game_path.starts_with("textures/") {
                            out.insert(strip_dds(&j.game_path).to_string());
                        }
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// Mesh folders for a `nif_filter` picker (`architecture/whiterun`).
    pub fn known_mesh_dirs(&self) -> &[String] {
        &self.known.mesh_dirs
    }
}
