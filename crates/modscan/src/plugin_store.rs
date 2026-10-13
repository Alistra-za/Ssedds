//! Plugin tables of the scan store (esp scanner spec §2.3): the per-plugin cache rows and the load-order resolution.
use crate::plugins::{clamp_local, is_light};
use crate::store::{path_id, Store};
use crate::ScanError;
use esp::PluginFacts;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// Where a plugin came from. `size` is `None` after a failed read, so the row is never reused (read again next scan).
#[derive(Debug, Clone)]
pub struct PluginFile<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub size: Option<u64>,
    pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginRow {
    pub id: i64,
    /// As on disk.
    pub name: String,
    pub path: String,
    pub size: Option<u64>,
    pub mtime: i64,
    pub lo_index: Option<i64>,
    pub light: bool,
    pub masters: Vec<String>,
    /// The parser's errors, one per line.
    pub error_text: Option<String>,
}

/// Tables with plugin rows, for deletes.
const PLUGIN_TABLES: [&str; 6] = [
    "rec",
    "txst_slot",
    "model_ref",
    "alt_tex",
    "txst_ref",
    "tex_ref",
];

/// (table, load-order column, local column) recomputed by `resolve_plugins`.
const REMAP: [(&str, &str, &str); 8] = [
    ("rec", "formid", "local"),
    ("txst_slot", "formid", "local"),
    ("model_ref", "formid", "local"),
    ("tex_ref", "formid", "local"),
    ("alt_tex", "formid", "local"),
    ("alt_tex", "txst_formid", "txst_local"),
    ("txst_ref", "formid", "local"),
    ("txst_ref", "txst_formid", "txst_local"),
];

fn sig_text(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn json_err(e: serde_json::Error) -> ScanError {
    ScanError::Io(std::io::Error::other(e))
}

fn delete_rows(c: &Connection, plugin_id: i64) -> rusqlite::Result<()> {
    for t in PLUGIN_TABLES {
        c.prepare_cached(&format!("DELETE FROM {t} WHERE plugin_id = ?1"))?
            .execute([plugin_id])?;
    }
    Ok(())
}

impl Store {
    /// Replaces one plugin's rows with `facts` (local FormIDs, `lo_index` NULL until `resolve_plugins`).
    pub fn write_plugin(&mut self, f: &PluginFile, facts: &PluginFacts) -> Result<i64, ScanError> {
        let tx = self.conn.transaction()?;
        let masters = serde_json::to_string(&facts.masters).map_err(json_err)?;
        let errors = (!facts.errors.is_empty()).then(|| facts.errors.join("\n"));
        let id: i64 = tx.query_row(
            "INSERT INTO plugin(name, path, size, mtime, lo_index, light, flags, masters_json, error_text)
             VALUES (?1,?2,?3,?4,NULL,?5,?6,?7,?8)
             ON CONFLICT(name) DO UPDATE SET name = excluded.name, path = excluded.path, size = excluded.size,
               mtime = excluded.mtime, lo_index = NULL, light = excluded.light, flags = excluded.flags,
               masters_json = excluded.masters_json, error_text = excluded.error_text
             RETURNING id",
            params![
                f.name,
                f.path,
                f.size.map(|s| s as i64),
                f.mtime,
                is_light(f.name, facts.flags),
                i64::from(facts.flags),
                masters,
                errors
            ],
            |r| r.get(0),
        )?;
        delete_rows(&tx, id)?;
        let nm = facts.masters.len();
        for r in &facts.records {
            let local = i64::from(clamp_local(r.local_formid, nm));
            let t = r.payload.txst.as_ref();
            tx.prepare_cached(
                "INSERT INTO rec(local, plugin_id, sig, edid, txst_flags, decal) VALUES (?1,?2,?3,?4,?5,?6)",
            )?
            .execute(params![
                local,
                id,
                sig_text(&r.sig),
                r.edid,
                t.map(|t| i64::from(t.flags)),
                t.map(|t| t.decal)
            ])?;
            if let Some(t) = t {
                for (slot, p) in t.slots.iter().enumerate() {
                    if let Some(p) = p {
                        let pid = path_id(&tx, p)?;
                        tx.prepare_cached(
                            "INSERT INTO txst_slot(local, plugin_id, slot, path_id) VALUES (?1,?2,?3,?4)",
                        )?
                        .execute(params![local, id, slot as i64, pid])?;
                    }
                }
            }
            for m in &r.payload.models {
                let pid = path_id(&tx, &m.path)?;
                tx.prepare_cached(
                    "INSERT INTO model_ref(local, plugin_id, slot, path_id) VALUES (?1,?2,?3,?4)",
                )?
                .execute(params![local, id, i64::from(m.slot), pid])?;
                for a in &m.alt {
                    tx.prepare_cached(
                        "INSERT INTO alt_tex(local, plugin_id, model_slot, shape_name, txst_local, idx)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                    )?
                    .execute(params![
                        local,
                        id,
                        i64::from(m.slot),
                        a.shape_name,
                        i64::from(clamp_local(a.txst_local_formid, nm)),
                        i64::from(a.index)
                    ])?;
                }
            }
            for (role, t) in &r.payload.txst_refs {
                tx.prepare_cached(
                    "INSERT INTO txst_ref(local, plugin_id, role, txst_local) VALUES (?1,?2,?3,?4)",
                )?
                .execute(params![
                    local,
                    id,
                    sig_text(role),
                    i64::from(clamp_local(*t, nm))
                ])?;
            }
            for (role, p) in &r.payload.textures {
                let pid = path_id(&tx, p)?;
                tx.prepare_cached(
                    "INSERT INTO tex_ref(local, plugin_id, role, path_id) VALUES (?1,?2,?3,?4)",
                )?
                .execute(params![local, id, sig_text(role), pid])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Deletes every plugin (with its rows) whose name is not in `active`; returns how many.
    pub fn retain_plugins(&mut self, active: &[String]) -> Result<usize, ScanError> {
        let keep: HashSet<String> = active.iter().map(|n| n.to_ascii_lowercase()).collect();
        let tx = self.conn.transaction()?;
        let gone: Vec<i64> = {
            let mut st = tx.prepare("SELECT id, name FROM plugin")?;
            let all = st
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            all.into_iter()
                .filter(|(_, n)| !keep.contains(&n.to_ascii_lowercase()))
                .map(|(id, _)| id)
                .collect()
        };
        for id in &gone {
            delete_rows(&tx, *id)?;
            tx.execute("DELETE FROM plugin WHERE id = ?1", [id])?;
        }
        if !gone.is_empty() {
            // Same transaction as the removal: a failed resolution then cannot leave a stale order that looks resolved.
            tx.execute_batch("UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner;")?;
        }
        tx.commit()?;
        Ok(gone.len())
    }

    /// Every stored plugin by lowercase name.
    pub fn plugin_rows(&self) -> Result<HashMap<String, PluginRow>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT id, name, path, size, mtime, lo_index, light, masters_json, error_text FROM plugin",
        )?;
        let rows = st.query_map([], |r| {
            Ok(PluginRow {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                size: r.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                mtime: r.get(4)?,
                lo_index: r.get(5)?,
                light: r.get(6)?,
                masters: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                error_text: r.get(8)?,
            })
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let row = row?;
            out.insert(row.name.to_ascii_lowercase(), row);
        }
        Ok(out)
    }

    /// Load-order FormIDs and winners (ruling R1/R2). `lo`: (plugin id, load-order position) of every active plugin.
    /// `map`: [plugin id, master index, base, mask] for every master index that is loaded, the plugin itself
    /// included (index = its master count). An unmapped index keeps the raw local id with `owner_ok` = 0.
    pub fn resolve_plugins(
        &mut self,
        lo: &[(i64, i64)],
        map: &[[i64; 4]],
    ) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS lo_map (plugin_id INTEGER NOT NULL, idx INTEGER NOT NULL,
               base INTEGER NOT NULL, mask INTEGER NOT NULL, PRIMARY KEY(plugin_id, idx)) WITHOUT ROWID;
             DELETE FROM lo_map;
             UPDATE plugin SET lo_index = NULL;",
        )?;
        {
            let mut m = tx.prepare("INSERT OR REPLACE INTO lo_map VALUES (?1,?2,?3,?4)")?;
            for r in map {
                m.execute(params![r[0], r[1], r[2], r[3]])?;
            }
            let mut l = tx.prepare("UPDATE plugin SET lo_index = ?2 WHERE id = ?1")?;
            for (id, i) in lo {
                l.execute(params![id, i])?;
            }
        }
        for (t, out, src) in REMAP {
            tx.execute(
                &format!(
                    "UPDATE {t} SET {out} = coalesce((SELECT m.base | ({t}.{src} & m.mask) FROM lo_map m
                       WHERE m.plugin_id = {t}.plugin_id AND m.idx = ({t}.{src} >> 24)), {t}.{src})"
                ),
                [],
            )?;
        }
        // SQLite: with one max() aggregate, the bare columns come from the row holding the max (the last plugin).
        tx.execute_batch(
            "UPDATE rec SET owner_ok = EXISTS(SELECT 1 FROM lo_map m WHERE m.plugin_id = rec.plugin_id
               AND m.idx = (rec.local >> 24));
             DELETE FROM rec_winner;
             INSERT INTO rec_winner(formid, plugin_id, n_overrides)
               SELECT formid, plugin_id, n FROM (
                 SELECT r.formid AS formid, r.plugin_id AS plugin_id, max(p.lo_index) AS lo, count(*) - 1 AS n
                 FROM rec r JOIN plugin p ON p.id = r.plugin_id
                 WHERE r.owner_ok = 1 AND p.lo_index IS NOT NULL GROUP BY r.formid);",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A Folder scan (ruling R7): no load order, so no winners; the rows stay as cache and the next MO2 scan
    /// re-resolves them (`lo_index` NULL never matches).
    pub fn unload_plugins(&mut self) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner;")?;
        tx.commit()?;
        Ok(())
    }

    pub fn plugin_record_count(&self) -> Result<usize, ScanError> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM rec", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}
