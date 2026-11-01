//! Plugin tables of the scan store (esp scanner spec §2.3): the per-plugin cache rows and the load-order resolution.
use crate::plugins::{clamp_local, is_light};
use crate::store::{intern_path, tex_query, Store, TexKey};
use crate::ScanError;
use esp::PluginFacts;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

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
const PLUGIN_TABLES: [&str; 7] = [
    "rec",
    "txst_slot",
    "model_ref",
    "alt_tex",
    "txst_ref",
    "tex_ref",
    "placed",
];

/// (table, load-order column, local column) recomputed by `resolve_plugins`.
const REMAP: [(&str, &str, &str); 9] = [
    ("rec", "formid", "local"),
    ("txst_slot", "formid", "local"),
    ("model_ref", "formid", "local"),
    ("tex_ref", "formid", "local"),
    ("alt_tex", "formid", "local"),
    ("alt_tex", "txst_formid", "txst_local"),
    ("txst_ref", "formid", "local"),
    ("txst_ref", "txst_formid", "txst_local"),
    ("placed", "base_formid", "base_local"),
];

/// Filtering spec §3: which winning records nothing reaches, conservatively. Runs after `rec_winner` is built, in the
/// same transaction. `win` = each winning copy; edges and alternate textures count only from winning copies, matched by the copy's `local` (an unresolved copy
/// can share its FormID with the winner). An active
/// plugin with `error_text` may have an incomplete placement list, so then nothing is judged "not placed".
const INERT_SQL: &str = "
DELETE FROM rec_inert;
CREATE TEMP TABLE IF NOT EXISTS win (formid INTEGER PRIMARY KEY, plugin_id INTEGER NOT NULL, sig TEXT NOT NULL,
  hdpt_flags INTEGER, local INTEGER NOT NULL);
DELETE FROM win;
INSERT OR IGNORE INTO win SELECT w.formid, w.plugin_id, r.sig, r.hdpt_flags, r.local FROM rec_winner w
  JOIN rec r ON r.plugin_id = w.plugin_id AND r.formid = w.formid AND r.owner_ok = 1;
CREATE TEMP TABLE IF NOT EXISTS placed_base (formid INTEGER PRIMARY KEY);
DELETE FROM placed_base;
INSERT OR IGNORE INTO placed_base SELECT p.base_formid FROM placed p JOIN plugin pl ON pl.id = p.plugin_id
  WHERE pl.lo_index IS NOT NULL AND p.base_formid IS NOT NULL;
-- A faulted active plugin (parse error, or past the load-order limit so unmapped): pretend every TXST/placeable is placed.
INSERT OR IGNORE INTO placed_base SELECT formid FROM win
  WHERE EXISTS (SELECT 1 FROM plugin pl WHERE pl.lo_index IS NOT NULL AND (pl.error_text IS NOT NULL
    OR NOT EXISTS (SELECT 1 FROM lo_map m WHERE m.plugin_id = pl.id)));
-- 2 not placed: placeable bases no REFR/ACHR of the load order places (ruling R10).
INSERT INTO rec_inert SELECT formid, 2 FROM win
  WHERE sig IN ('STAT', 'MSTT', 'FURN', 'DOOR', 'CONT', 'LIGH', 'ACTI', 'TREE', 'FLOR')
    AND formid NOT IN (SELECT formid FROM placed_base);
-- 3 head part not playable: DATA without Playable (0x01; no DATA = playable) and no playable winning HDPT lists it
-- in HNAM (ruling R9).
INSERT INTO rec_inert SELECT h.formid, 3 FROM win h
  WHERE h.sig = 'HDPT' AND h.hdpt_flags & 1 = 0
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win p ON p.local = e.local AND p.plugin_id = e.plugin_id
      WHERE e.role = 'HNAM' AND e.txst_formid = h.formid AND p.sig = 'HDPT' AND ifnull(p.hdpt_flags, 1) & 1 = 1);
-- 1 no reference: grass no winning landscape texture lists (GNAM).
INSERT INTO rec_inert SELECT g.formid, 1 FROM win g
  WHERE g.sig = 'GRAS'
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win l ON l.local = e.local AND l.plugin_id = e.plugin_id
      WHERE e.role = 'GNAM' AND e.txst_formid = g.formid);
-- 1 no reference: TXSTs that no reference places and no winning, non-inert record uses (alternate textures; LTEX/HDPT
-- TNAM, NPC_ FTST, RACE face sets, ARMA NAM0/NAM1, IPCT DNAM/ENAM, PROJ decal, any FLST entry). MODL is ARMO -> ARMA, not a texture use.
INSERT INTO rec_inert SELECT t.formid, 1 FROM win t
  WHERE t.sig = 'TXST'
    AND t.formid NOT IN (SELECT formid FROM placed_base)
    AND NOT EXISTS (SELECT 1 FROM alt_tex a JOIN win u ON u.local = a.local AND u.plugin_id = a.plugin_id
      WHERE a.txst_formid = t.formid AND u.formid NOT IN (SELECT formid FROM rec_inert))
    AND NOT EXISTS (SELECT 1 FROM txst_ref e JOIN win u ON u.local = e.local AND u.plugin_id = e.plugin_id
      WHERE e.txst_formid = t.formid AND e.role NOT IN ('HNAM', 'GNAM', 'MODL')
        AND u.formid NOT IN (SELECT formid FROM rec_inert));";

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
        // Verdicts rest on the old rows; a cancelled pass must not keep them (resolve rebuilds them).
        tx.execute("DELETE FROM rec_inert", [])?;
        let nm = facts.masters.len();
        for r in &facts.records {
            let local = i64::from(clamp_local(r.local_formid, nm));
            let t = r.payload.txst.as_ref();
            tx.prepare_cached(
                "INSERT INTO rec(local, plugin_id, sig, edid, txst_flags, decal, hdpt_flags, mato_flags) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?
            .execute(params![
                local,
                id,
                sig_text(&r.sig),
                r.edid,
                t.map(|t| i64::from(t.flags)),
                t.map(|t| t.decal),
                r.payload.hdpt_flags.map(i64::from),
                r.payload.mato_flags.map(i64::from)
            ])?;
            if let Some(t) = t {
                for (slot, p) in t.slots.iter().enumerate() {
                    if let Some(p) = p {
                        let pid = intern_path(&tx, &mut self.intern, p)?;
                        tx.prepare_cached(
                            "INSERT INTO txst_slot(local, plugin_id, slot, path_id) VALUES (?1,?2,?3,?4)",
                        )?
                        .execute(params![local, id, slot as i64, pid])?;
                    }
                }
            }
            for m in &r.payload.models {
                let pid = intern_path(&tx, &mut self.intern, &m.path)?;
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
                let pid = intern_path(&tx, &mut self.intern, p)?;
                tx.prepare_cached(
                    "INSERT INTO tex_ref(local, plugin_id, role, path_id) VALUES (?1,?2,?3,?4)",
                )?
                .execute(params![local, id, sig_text(role), pid])?;
            }
        }
        for &base in &facts.placed {
            tx.prepare_cached("INSERT INTO placed(plugin_id, base_local) VALUES (?1,?2)")?
                .execute(params![id, i64::from(clamp_local(base, nm))])?;
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
            tx.execute_batch(
                "UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner; DELETE FROM rec_inert;",
            )?;
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

    /// The stored file path of plugin `name` (ASCII case-insensitive, as plugin names are); `None` when not stored.
    pub fn plugin_path(&self, name: &str) -> Result<Option<PathBuf>, ScanError> {
        Ok(self
            .conn
            .query_row(
                "SELECT path FROM plugin WHERE name = ?1 COLLATE NOCASE",
                [name],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(PathBuf::from))
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
            // A target whose master is missing stays NULL: its raw local id could equal a real winner's FormID.
            let fallback = if out == "formid" {
                format!("{t}.{src}")
            } else {
                "NULL".to_string()
            };
            tx.execute(
                &format!(
                    "UPDATE {t} SET {out} = coalesce((SELECT m.base | ({t}.{src} & m.mask) FROM lo_map m
                       WHERE m.plugin_id = {t}.plugin_id AND m.idx = ({t}.{src} >> 24)), {fallback})"
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
        tx.execute_batch(INERT_SQL)?;
        tx.commit()?;
        Ok(())
    }

    /// A Folder scan (ruling R7): no load order, so no winners; the rows stay as cache and the next MO2 scan
    /// re-resolves them (`lo_index` NULL never matches).
    pub fn unload_plugins(&mut self) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "UPDATE plugin SET lo_index = NULL; DELETE FROM rec_winner; DELETE FROM rec_inert;",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn plugin_record_count(&self) -> Result<usize, ScanError> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM rec", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Complex grass design §5 condition 1: slot-0 textures of the lighting shapes of the winning NIF that the `MODL`
    /// (model slot 0) of each winning GRAS record names, in an enabled origin (a derived winner is read through its
    /// source copy; TruePBR shapes are skipped). Store form: lower case, `/`.
    pub fn gras_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError> {
        self.gras_slot0_textures(false)
    }

    /// PBR grass design §4.2 (ruling R5): [`Self::gras_diffuse_textures`] with TruePBR (SLSF2.23) shapes kept, so a
    /// pre-patched PBR grass NIF counts as well as a vanilla one: the evidence for the `pbr_grass` auto-pick.
    pub fn gras_all_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError> {
        self.gras_slot0_textures(true)
    }

    fn gras_slot0_textures(&self, keep_truepbr: bool) -> Result<BTreeSet<String>, ScanError> {
        let truepbr = if keep_truepbr {
            ""
        } else {
            " AND (ifnull(s.slsf2,0) & 8388608) = 0"
        };
        let mut st = self.conn.prepare(&format!(
            // CROSS JOIN pins the join order (SQLite never reorders it): start from the few GRAS records. Left to
            // the planner (no ANALYZE stats) it scanned `path` first and hung the GUI for minutes on a real modlist.
            "SELECT DISTINCT tp.p
             FROM rec r
             CROSS JOIN model_ref m ON m.plugin_id = r.plugin_id AND m.local = r.local AND m.slot = 0
             CROSS JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id
             CROSS JOIN nif_winner nw ON nw.path_id = m.path_id
             CROSS JOIN nif_copy wc ON wc.id = nw.copy_id
             CROSS JOIN origin wo ON wo.id = wc.origin_id AND wo.enabled = 1
             LEFT JOIN nif_winner_src ws ON ws.path_id = nw.path_id
             CROSS JOIN nif_copy c ON c.id = CASE WHEN wc.is_derived = 1
                                            THEN coalesce(ws.copy_id, wc.source_copy_id, wc.id) ELSE wc.id END
             CROSS JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             CROSS JOIN shape s ON s.copy_id = c.id AND s.shader_kind = 1{truepbr}
             CROSS JOIN tex_use tu ON tu.texset_id = s.texset_id AND tu.slot = 0
             CROSS JOIN path tp ON tp.id = tu.tex_path_id
             WHERE r.sig = 'GRAS' AND r.owner_ok = 1"
        ))?;
        let v = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
}

/// Records listed per missing path (spec §4.4).
pub const MISSING_RECORDS: usize = 5;

/// The text of a `rec_inert.reason` (ruling R13).
pub fn inert_reason(code: i64) -> &'static str {
    match code {
        2 => "not placed",
        3 => "head part not playable",
        _ => "no reference",
    }
}

/// One path's rows gathered by `plugin_missing_with_inert`: live labels, inert labels.
type MissingAcc = (String, Vec<String>, Vec<String>);

fn push_missing(
    c: Option<MissingAcc>,
    live: &mut Vec<MissingRef>,
    inert_refs: &mut Vec<MissingRef>,
) {
    let Some((path, l, i)) = c else {
        return;
    };
    let (to, labels) = if l.is_empty() {
        (inert_refs, i)
    } else {
        (live, l)
    };
    to.push(MissingRef {
        path,
        n_records: labels.len(),
        records: labels.into_iter().take(MISSING_RECORDS).collect(),
    });
}

/// One record as listed everywhere: `EDID [FORMID] SIG — Plugin`.
pub fn record_label(edid: Option<&str>, formid: u32, sig: &str, plugin: &str) -> String {
    format!(
        "{} [{formid:08X}] {sig} — {plugin}",
        edid.unwrap_or("(no EDID)")
    )
}

/// A winning TXST whose slot matches a set's key.
#[derive(Debug, Clone, PartialEq)]
pub struct TxstUse {
    pub formid: u32,
    pub edid: Option<String>,
    /// The winning plugin.
    pub plugin: String,
    /// The NIF `BSShaderTextureSet` slot, as the set editor numbers it (not the TXST `TX0n` index).
    pub slot: u32,
    pub path: String,
    pub n_overrides: u32,
}

/// One alternate-texture entry: a NIF shape re-textured with a TXST.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeSwap {
    pub shape_name: String,
    pub txst_formid: u32,
    /// The EDID of the TXST's winning copy.
    pub txst_edid: Option<String>,
    pub index: i32,
}

/// A winning record whose alternate textures use a given TXST.
#[derive(Debug, Clone, PartialEq)]
pub struct AltUse {
    pub formid: u32,
    pub edid: Option<String>,
    pub sig: String,
    pub plugin: String,
    pub model_slot: u32,
    pub swap: ShapeSwap,
}

/// A winning record that uses a mesh, with its swaps for that model slot.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshUse {
    pub formid: u32,
    pub edid: Option<String>,
    pub sig: String,
    pub plugin: String,
    pub model_slot: u32,
    pub n_overrides: u32,
    pub swaps: Vec<ShapeSwap>,
}

/// A path winning plugin records name that nothing provides (spec §4.4).
#[derive(Debug, Clone, PartialEq)]
pub struct MissingRef {
    pub path: String,
    pub n_records: usize,
    /// The first [`MISSING_RECORDS`], as [`record_label`].
    pub records: Vec<String>,
}

/// A winning record of one type that has an EDID (design §6): the material-object picker and the EDID checks.
#[derive(Debug, Clone, PartialEq)]
pub struct WinRec {
    pub formid: u32,
    pub edid: String,
    /// The winning plugin.
    pub plugin: String,
    /// MATO `DATA` flags of the winning copy (bit 0 = single pass, at offset 44); `None` when DATA is under 48 bytes.
    pub mato_flags: Option<u32>,
}

/// `alt_tex a` with its TXST's winning copy (`tr`); select list per query.
const SWAP_JOINS: &str = "alt_tex a
     LEFT JOIN rec_winner tw ON tw.formid = a.txst_formid
     LEFT JOIN rec tr ON tr.plugin_id = tw.plugin_id AND tr.formid = tw.formid AND tr.owner_ok = 1";

fn swap_row(r: &rusqlite::Row, at: usize) -> rusqlite::Result<ShapeSwap> {
    Ok(ShapeSwap {
        shape_name: r.get(at)?,
        txst_formid: r.get::<_, Option<i64>>(at + 1)?.unwrap_or(0) as u32,
        txst_edid: r.get(at + 2)?,
        index: r.get::<_, i64>(at + 3)? as i32,
    })
}

/// No enabled origin (no completed scan) lists nothing: the negative query would otherwise call every path missing.
/// Paths of winning records (`txst_slot`, `tex_ref`, `model_ref`) that are `textures/**.dds` or `meshes/**.nif`, not
/// FaceGen, and that no enabled origin's winner provides (ruling R8). Column 6 is the record's inert reason (NULL = live).
const MISSING_SQL: &str = "
WITH refs(path_id, formid, plugin_id, local) AS (
  SELECT t.path_id, t.formid, t.plugin_id, t.local FROM txst_slot t
    JOIN rec_winner w ON w.formid = t.formid AND w.plugin_id = t.plugin_id
  UNION ALL SELECT t.path_id, t.formid, t.plugin_id, t.local FROM tex_ref t
    JOIN rec_winner w ON w.formid = t.formid AND w.plugin_id = t.plugin_id
  UNION ALL SELECT m.path_id, m.formid, m.plugin_id, m.local FROM model_ref m
    JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id)
SELECT DISTINCT p.p, r.edid, x.formid, r.sig, pl.name, i.reason FROM refs x
JOIN path p ON p.id = x.path_id
JOIN rec r ON r.plugin_id = x.plugin_id AND r.local = x.local AND r.owner_ok = 1
JOIN plugin pl ON pl.id = x.plugin_id
LEFT JOIN rec_inert i ON i.formid = x.formid
WHERE EXISTS (SELECT 1 FROM origin WHERE enabled = 1)
  AND p.p NOT LIKE 'textures/actors/character/facegendata/%'
  AND p.p NOT LIKE 'meshes/actors/character/facegendata/%'
  AND ((p.p LIKE 'textures/%.dds' AND NOT EXISTS (SELECT 1 FROM tex_winner tw
          JOIN origin o ON o.id = tw.origin_id AND o.enabled = 1 WHERE tw.path_id = x.path_id))
    OR (p.p LIKE 'meshes/%.nif' AND NOT EXISTS (SELECT 1 FROM nif_winner nw JOIN nif_copy c ON c.id = nw.copy_id
          JOIN origin o ON o.id = c.origin_id AND o.enabled = 1 WHERE nw.path_id = x.path_id)))
ORDER BY p.p, pl.name, x.formid";

/// Winning textures of the run that no NIF copy of the run and no record copy of an active plugin names, outside
/// `textures/pbr/` (ruling R8). A record that is inert (filtering spec §3) is no use.
const UNUSED_SQL: &str = "
SELECT p.p FROM tex_winner tw JOIN origin o ON o.id = tw.origin_id AND o.enabled = 1
JOIN path p ON p.id = tw.path_id
WHERE p.p NOT LIKE 'textures/pbr/%'
  AND NOT EXISTS (SELECT 1 FROM tex_use tu JOIN shape s ON s.texset_id = tu.texset_id
        JOIN nif_copy c ON c.id = s.copy_id JOIN origin co ON co.id = c.origin_id AND co.enabled = 1
        WHERE tu.tex_path_id = tw.path_id)
  AND NOT EXISTS (SELECT 1 FROM txst_slot t JOIN plugin pl ON pl.id = t.plugin_id AND pl.lo_index IS NOT NULL
        WHERE t.path_id = tw.path_id
          AND NOT EXISTS (SELECT 1 FROM rec_inert i WHERE i.formid = t.formid))
  AND NOT EXISTS (SELECT 1 FROM tex_ref t JOIN plugin pl ON pl.id = t.plugin_id AND pl.lo_index IS NOT NULL
        WHERE t.path_id = tw.path_id
          AND NOT EXISTS (SELECT 1 FROM rec_inert i WHERE i.formid = t.formid))
ORDER BY p.p";

/// TXST `TX0n` for each NIF `BSShaderTextureSet` slot (TX02 = env mask = NIF 5, TX03 = glow = NIF 2, ...), and back.
const NIF_TO_TX: [i64; 8] = [0, 1, 3, 4, 5, 2, 6, 7];
const TX_TO_NIF: [u32; 8] = [0, 1, 5, 2, 3, 4, 6, 7];

impl Store {
    /// Winning TXSTs whose slot matches `key` with PGPatcher's rule (ruling R9), by EDID; at most `limit`.
    pub fn txsts_matching(&self, key: &TexKey, limit: usize) -> Result<Vec<TxstUse>, ScanError> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let Some((slot, cond, arg, check)) = tex_query(key, "tp.p") else {
            return Ok(vec![]);
        };
        let Some(&tx_slot) = NIF_TO_TX.get(slot as usize) else {
            return Ok(vec![]);
        };
        let mut st = self.conn.prepare(&format!(
            "SELECT ts.formid, r.edid, pl.name, ts.slot, tp.p, w.n_overrides FROM path tp
             JOIN txst_slot ts ON ts.path_id = tp.id AND ts.slot = ?1
             JOIN rec_winner w ON w.formid = ts.formid AND w.plugin_id = ts.plugin_id
             JOIN rec r ON r.plugin_id = ts.plugin_id AND r.local = ts.local AND r.owner_ok = 1
             JOIN plugin pl ON pl.id = ts.plugin_id
             WHERE {cond} ORDER BY r.edid, ts.formid"
        ))?;
        let mut rows = st.query(params![tx_slot, arg])?;
        let mut out = vec![];
        while let Some(r) = rows.next()? {
            let path: String = r.get(4)?;
            if !check(&path) {
                continue;
            }
            out.push(TxstUse {
                formid: r.get::<_, i64>(0)? as u32,
                edid: r.get(1)?,
                plugin: r.get(2)?,
                slot: {
                    let tx = r.get::<_, i64>(3)? as u32;
                    TX_TO_NIF.get(tx as usize).copied().unwrap_or(tx)
                },
                path,
                n_overrides: r.get::<_, i64>(5)? as u32,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Winning records whose alternate textures use one of `txsts` (load-order FormIDs).
    pub fn alt_users_of(&self, txsts: &[u32]) -> Result<Vec<AltUse>, ScanError> {
        let ids = serde_json::to_string(txsts).map_err(json_err)?;
        let mut st = self.conn.prepare(&format!(
            "SELECT a.formid, r.edid, r.sig, pl.name, a.model_slot, a.shape_name, a.txst_formid, tr.edid, a.idx
             FROM {SWAP_JOINS}
             JOIN rec_winner w ON w.formid = a.formid AND w.plugin_id = a.plugin_id
             JOIN rec r ON r.plugin_id = a.plugin_id AND r.local = a.local AND r.owner_ok = 1
             JOIN plugin pl ON pl.id = a.plugin_id
             WHERE a.txst_formid IN (SELECT value FROM json_each(?1))
             ORDER BY r.edid, a.formid, a.model_slot, a.idx"
        ))?;
        let v = st
            .query_map([ids], |r| {
                Ok(AltUse {
                    formid: r.get::<_, i64>(0)? as u32,
                    edid: r.get(1)?,
                    sig: r.get(2)?,
                    plugin: r.get(3)?,
                    model_slot: r.get::<_, i64>(4)? as u32,
                    swap: swap_row(r, 5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// Winning records whose model (any slot) is `norm`, with the swaps of that model slot (spec §4.2).
    pub fn plugin_uses_of_mesh(&self, norm: &str) -> Result<Vec<MeshUse>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT m.formid, r.edid, r.sig, pl.name, m.slot, w.n_overrides, m.plugin_id, m.local
             FROM path p JOIN model_ref m ON m.path_id = p.id
             JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id
             JOIN rec r ON r.plugin_id = m.plugin_id AND r.local = m.local AND r.owner_ok = 1
             JOIN plugin pl ON pl.id = m.plugin_id
             WHERE p.p = ?1 ORDER BY r.sig, r.edid, m.formid, m.slot",
        )?;
        let rows: Vec<(MeshUse, i64, i64)> = st
            .query_map([norm], |r| {
                Ok((
                    MeshUse {
                        formid: r.get::<_, i64>(0)? as u32,
                        edid: r.get(1)?,
                        sig: r.get(2)?,
                        plugin: r.get(3)?,
                        model_slot: r.get::<_, i64>(4)? as u32,
                        n_overrides: r.get::<_, i64>(5)? as u32,
                        swaps: vec![],
                    },
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut sw = self.conn.prepare(&format!(
            "SELECT a.shape_name, a.txst_formid, tr.edid, a.idx FROM {SWAP_JOINS} WHERE a.plugin_id = ?1 AND a.local = ?2 AND a.model_slot = ?3 ORDER BY a.idx, a.shape_name"
        ))?;
        let mut out = Vec::with_capacity(rows.len());
        for (mut u, plugin_id, local) in rows {
            u.swaps = sw
                .query_map(params![plugin_id, local, i64::from(u.model_slot)], |r| {
                    swap_row(r, 0)
                })?
                .collect::<Result<_, _>>()?;
            out.push(u);
        }
        Ok(out)
    }

    /// The mesh `norm` (canonical) is a head part (Note BA): under `meshes/actors/character/facegendata/`, or the
    /// MODL of any HDPT copy in the scanned plugins (not only winners: an overridden head part's mesh is still a head).
    pub fn is_head_part_mesh(&self, norm: &str) -> Result<bool, ScanError> {
        if nif::path::is_facegen(norm) {
            return Ok(true);
        }
        let mut st = self.conn.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM path p JOIN model_ref m ON m.path_id = p.id
             JOIN rec r ON r.plugin_id = m.plugin_id AND r.local = m.local
             WHERE p.p = ?1 AND r.sig = 'HDPT')",
        )?;
        Ok(st.query_row([norm], |r| r.get(0))?)
    }

    /// Missing files (spec §4.4, ruling R8) that at least one live record names, by path (filtering ruling R12).
    pub fn plugin_missing_files(&self) -> Result<Vec<MissingRef>, ScanError> {
        Ok(self.plugin_missing_with_inert()?.0)
    }

    /// Missing files split by who names them (filtering ruling R12): `.0` paths a live record names (only live records
    /// listed), `.1` paths only inert records name, each label ending in ` (reason)`.
    pub fn plugin_missing_with_inert(
        &self,
    ) -> Result<(Vec<MissingRef>, Vec<MissingRef>), ScanError> {
        let mut st = self.conn.prepare(MISSING_SQL)?;
        let mut rows = st.query([])?;
        let (mut live, mut inert_refs) = (vec![], vec![]);
        let mut cur: Option<MissingAcc> = None;
        while let Some(r) = rows.next()? {
            let path: String = r.get(0)?;
            let edid: Option<String> = r.get(1)?;
            let label = record_label(
                edid.as_deref(),
                r.get::<_, i64>(2)? as u32,
                &r.get::<_, String>(3)?,
                &r.get::<_, String>(4)?,
            );
            let reason: Option<i64> = r.get(5)?;
            if cur.as_ref().is_some_and(|c| c.0 != path) {
                push_missing(cur.take(), &mut live, &mut inert_refs);
            }
            let c = cur.get_or_insert_with(|| (path, vec![], vec![]));
            match reason {
                None => c.1.push(label),
                Some(x) => c.2.push(format!("{label} ({})", inert_reason(x))),
            }
        }
        push_missing(cur, &mut live, &mut inert_refs);
        Ok((live, inert_refs))
    }

    /// The inert reason (filtering spec §3) of each of `formids` that is inert.
    pub fn inert_of(&self, formids: &[u32]) -> Result<HashMap<u32, &'static str>, ScanError> {
        let ids = serde_json::to_string(formids).map_err(json_err)?;
        let mut st = self.conn.prepare(
            "SELECT formid, reason FROM rec_inert WHERE formid IN (SELECT value FROM json_each(?1))",
        )?;
        let v = st
            .query_map([ids], |r| {
                Ok((r.get::<_, i64>(0)? as u32, inert_reason(r.get(1)?)))
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// Unused textures (spec §4.4, ruling R8), by path.
    pub fn unused_textures(&self) -> Result<Vec<String>, ScanError> {
        let mut st = self.conn.prepare(UNUSED_SQL)?;
        let v: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v.into_iter()
            .filter(|p| !nif::path::is_lod_path(p))
            .collect())
    }

    /// Winning records of type `sig` (`"TXST"`, `"MATO"`) that have an EDID, by EDID then FormID.
    pub fn winning_records(&self, sig: &str) -> Result<Vec<WinRec>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT w.formid, r.edid, pl.name, r.mato_flags FROM rec r
             JOIN rec_winner w ON w.formid = r.formid AND w.plugin_id = r.plugin_id
             JOIN plugin pl ON pl.id = r.plugin_id
             WHERE r.sig = ?1 AND r.owner_ok = 1 AND r.edid IS NOT NULL
             ORDER BY r.edid, w.formid",
        )?;
        let v = st
            .query_map([sig], |r| {
                Ok(WinRec {
                    formid: r.get::<_, i64>(0)? as u32,
                    edid: r.get(1)?,
                    plugin: r.get(2)?,
                    mato_flags: r.get::<_, Option<i64>>(3)?.map(|f| f as u32),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
}
