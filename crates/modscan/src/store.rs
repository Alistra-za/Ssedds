//! SQLite scan store (plan 02 §6 with the PGPatcher columns of 05 §5.2). One row per NIF copy and per shape.
use crate::ScanError;
use nif::scan::{NifScan, Shader};
use nif::NifError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS path   (id INTEGER PRIMARY KEY, p TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS origin (id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, name TEXT NOT NULL, root TEXT NOT NULL,
                                   parent_origin INTEGER, priority INTEGER NOT NULL, enabled INTEGER NOT NULL,
                                   derived_by INTEGER, derived_state INTEGER, UNIQUE(kind, root));
CREATE TABLE IF NOT EXISTS nif_copy (id INTEGER PRIMARY KEY, path_id INTEGER NOT NULL, origin_id INTEGER NOT NULL,
                                   size INTEGER, mtime INTEGER, crc32 INTEGER, parse_status INTEGER NOT NULL,
                                   bsver INTEGER, le_mesh INTEGER NOT NULL DEFAULT 0, is_derived INTEGER NOT NULL DEFAULT 0,
                                   source_copy_id INTEGER, derive_check INTEGER, problem TEXT,
                                   UNIQUE(path_id, origin_id));
CREATE TABLE IF NOT EXISTS shape (copy_id INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT, kind TEXT NOT NULL,
                                   shader_kind INTEGER NOT NULL, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                                   texset_id INTEGER, alpha_flags INTEGER, alpha_threshold INTEGER, params TEXT,
                                   PRIMARY KEY(copy_id, block)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS texset (id INTEGER PRIMARY KEY, n INTEGER NOT NULL,
                                   t0 INTEGER, t1 INTEGER, t2 INTEGER, t3 INTEGER, t4 INTEGER, t5 INTEGER, t6 INTEGER,
                                   t7 INTEGER, t8 INTEGER, UNIQUE(n, t0, t1, t2, t3, t4, t5, t6, t7, t8));
CREATE TABLE IF NOT EXISTS tex_use (tex_path_id INTEGER, texset_id INTEGER, slot INTEGER,
                                   PRIMARY KEY(tex_path_id, texset_id, slot)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS nif_winner     (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL, n_copies INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS nif_winner_src (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS pgp_diff (path_id INTEGER PRIMARY KEY, crc_orig INTEGER NOT NULL, crc_patched INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS nif_copy_origin ON nif_copy(origin_id);
-- SQLite treats NULLs as distinct in UNIQUE, so the texset UNIQUE above never dedups a set with an empty slot.
-- This expression index makes INSERT OR IGNORE dedup sets with empty (NULL) slots too (path ids start at 1).
CREATE UNIQUE INDEX IF NOT EXISTS texset_key ON texset(n, ifnull(t0,0), ifnull(t1,0), ifnull(t2,0), ifnull(t3,0),
                                   ifnull(t4,0), ifnull(t5,0), ifnull(t6,0), ifnull(t7,0), ifnull(t8,0));
CREATE INDEX IF NOT EXISTS shape_texset ON shape(texset_id);
"#;

const TEMP: &str = r#"
CREATE TEMP TABLE IF NOT EXISTS pending_derived (pid INTEGER PRIMARY KEY, path_id INTEGER NOT NULL,
                                   origin_id INTEGER NOT NULL, size INTEGER, mtime INTEGER, crc32 INTEGER,
                                   parse_status INTEGER NOT NULL, bsver INTEGER, le_mesh INTEGER NOT NULL, problem TEXT,
                                   UNIQUE(path_id, origin_id));
CREATE TEMP TABLE IF NOT EXISTS pending_shape (pid INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT, kind TEXT NOT NULL,
                                   shader_kind INTEGER NOT NULL, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                                   texset_id INTEGER, alpha_flags INTEGER, alpha_threshold INTEGER, params TEXT,
                                   PRIMARY KEY(pid, block));
DELETE FROM pending_shape;
DELETE FROM pending_derived;
"#;

/// `origin.kind`. 02 §6 used 3 for BSAs; the controller gave 3 to separators, so BSAs are 4.
pub const KIND_DATA: i32 = 0;
pub const KIND_MOD: i32 = 1;
pub const KIND_OVERWRITE: i32 = 2;
pub const KIND_SEPARATOR: i32 = 3;
pub const KIND_BSA: i32 = 4;

/// `nif_copy.parse_status`.
pub const PARSE_OK: i32 = 0;
pub const PARSE_PARTIAL: i32 = 1;
pub const PARSE_NOT_SKYRIM: i32 = 2;
pub const PARSE_FAILED: i32 = 3;

/// `origin.derived_state` (05 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedState {
    Ok = 0,
    Incomplete = 1,
    RunningAtScan = 2,
    InOverwrite = 3,
}

/// `nif_copy.derive_check` as stored (integer) and shown (string).
pub fn check_code(c: crate::pgp::DeriveCheck) -> i32 {
    use crate::pgp::DeriveCheck::*;
    match c {
        Match => 0,
        StaleChanged => 1,
        StaleReordered => 2,
        Unverified => 3,
        UnknownSource => 4,
        DupOf => 5,
    }
}

pub fn check_name(code: i32) -> Option<&'static str> {
    Some(match code {
        0 => "match",
        1 => "stale_changed",
        2 => "stale_reordered",
        3 => "unverified",
        4 => "unknown",
        5 => "dup_of",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModRow {
    pub id: i64,
    pub name: String,
    pub kind: i32,
    pub priority: i32,
    pub derived: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NifRow {
    pub copy_id: i64,
    pub path: String,
    pub wins: bool,
    /// The non-derived copy that beats this one (05 §5.3); for a losing derived copy, the game winner's origin.
    pub overridden_by: Option<String>,
    pub n_copies: u32,
    pub derived: bool,
    /// Own check for a derived copy; for a source copy patched in game, the check of the derived winner.
    pub derive_check: Option<String>,
    pub le_mesh: bool,
    pub problem: Option<String>,
    /// The game winner is a derived copy whose source copy is this copy.
    pub patched_in_game: bool,
}

impl NifRow {
    pub fn status_label(&self) -> &'static str {
        if self.wins {
            "Wins"
        } else if self.patched_in_game {
            "Patched by PGPatcher (in game)"
        } else {
            "Overridden"
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeView {
    pub block: u32,
    pub name: Option<String>,
    /// Shape block type, e.g. `BSTriShape`.
    pub kind: String,
    /// 0 none, 1 BSLightingShaderProperty, 2 BSEffectShaderProperty, 3 other shader block.
    pub shader_kind: i32,
    pub shader_type: Option<u32>,
    pub slsf1: Option<u32>,
    pub slsf2: Option<u32>,
    /// Lighting: texture-set slots (count read from the file). Effect: [source, greyscale]. `None` = empty slot.
    pub textures: Vec<Option<String>>,
    pub alpha_flags: Option<u16>,
    pub alpha_threshold: Option<u8>,
    /// `serde_json` of `LightingParams`/`EffectParams`; `{"shader":"<block type>"}` for other shaders.
    pub params: Option<String>,
}

/// Outcome of reading and scanning one copy.
#[derive(Debug, Clone)]
pub enum CopyResult {
    Scanned(NifScan),
    NotSkyrim {
        bsver: u32,
        msg: String,
    },
    /// The bytes were read but are not a valid NIF. Cached: reused while (size, mtime) are unchanged.
    Failed(String),
    /// The bytes could not be read (I/O, locked file, corrupt BSA entry). Stored with NULL size/mtime so the
    /// next scan never reuses the row and reads the file again.
    ReadFailed(String),
}

impl From<Result<NifScan, NifError>> for CopyResult {
    fn from(r: Result<NifScan, NifError>) -> Self {
        match r {
            Ok(s) => CopyResult::Scanned(s),
            Err(e @ NifError::NotSkyrim { bsver, .. }) => CopyResult::NotSkyrim {
                bsver,
                msg: e.to_string(),
            },
            Err(e) => CopyResult::Failed(e.to_string()),
        }
    }
}

impl CopyResult {
    /// (parse_status, bsver, le_mesh, problem)
    fn record(&self) -> (i32, Option<u32>, bool, Option<String>) {
        match self {
            CopyResult::Scanned(s) if s.problems.is_empty() => {
                (PARSE_OK, Some(s.bsver), s.le_mesh, None)
            }
            CopyResult::Scanned(s) => (
                PARSE_PARTIAL,
                Some(s.bsver),
                s.le_mesh,
                Some(s.problems.join("; ")),
            ),
            CopyResult::NotSkyrim { bsver, msg } => {
                (PARSE_NOT_SKYRIM, Some(*bsver), false, Some(msg.clone()))
            }
            CopyResult::Failed(m) | CopyResult::ReadFailed(m) => {
                (PARSE_FAILED, None, false, Some(m.clone()))
            }
        }
    }
}

/// One parsed copy to write in a chunk transaction.
#[derive(Debug)]
pub struct ChunkItem {
    pub path_id: i64,
    pub origin_id: i64,
    pub size: u64,
    pub mtime: i64,
    pub crc32: Option<u32>,
    /// Derived copies are staged in `pending_derived` until the post-walk PGPatcher check passes.
    pub derived: bool,
    pub result: CopyResult,
}

impl ChunkItem {
    fn read_failed(&self) -> bool {
        matches!(self.result, CopyResult::ReadFailed(_))
    }
    fn stored_size(&self) -> Option<i64> {
        (!self.read_failed()).then_some(self.size as i64)
    }
    fn stored_mtime(&self) -> Option<i64> {
        (!self.read_failed()).then_some(self.mtime)
    }
}

/// What the store already knows about a copy (for reuse by `(size, mtime)`).
#[derive(Debug, Clone)]
pub struct CopyMeta {
    pub id: i64,
    /// `None` after a read failure, so the copy is never reused and is re-read next scan.
    pub size: Option<u64>,
    pub mtime: Option<i64>,
    pub parse_status: i32,
    pub le_mesh: bool,
    pub problem: Option<String>,
    pub crc32: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct OriginSpec {
    pub kind: i32,
    pub name: String,
    pub root: String,
    pub parent: Option<i64>,
    pub priority: i32,
    pub derived_by: Option<i32>,
    pub derived_state: Option<DerivedState>,
}

/// Per-copy result of the derived-layer pass (step 6).
#[derive(Debug, Clone)]
pub struct DeriveRow {
    pub copy_id: i64,
    pub is_derived: bool,
    pub source_copy_id: Option<i64>,
    pub derive_check: Option<i32>,
}

pub struct Store {
    conn: Connection,
}

fn path_id(c: &Connection, p: &str) -> rusqlite::Result<i64> {
    c.prepare_cached("INSERT OR IGNORE INTO path(p) VALUES (?1)")?
        .execute([p])?;
    c.prepare_cached("SELECT id FROM path WHERE p = ?1")?
        .query_row([p], |r| r.get(0))
}

fn texset_id(c: &Connection, slots: &[Option<i64>]) -> rusqlite::Result<i64> {
    let n = slots.len().min(9);
    let mut t = [None::<i64>; 9];
    t[..n].copy_from_slice(&slots[..n]);
    c.prepare_cached(
        "INSERT OR IGNORE INTO texset(n,t0,t1,t2,t3,t4,t5,t6,t7,t8) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
    )?
    .execute(params![n as i64, t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8]])?;
    let k: Vec<i64> = t.iter().map(|x| x.unwrap_or(0)).collect();
    let id: i64 = c
        .prepare_cached(
            "SELECT id FROM texset WHERE n = ?1 AND ifnull(t0,0) = ?2 AND ifnull(t1,0) = ?3 AND ifnull(t2,0) = ?4 \
             AND ifnull(t3,0) = ?5 AND ifnull(t4,0) = ?6 AND ifnull(t5,0) = ?7 AND ifnull(t6,0) = ?8 \
             AND ifnull(t7,0) = ?9 AND ifnull(t8,0) = ?10",
        )?
        .query_row(
            params![n as i64, k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], k[8]],
            |r| r.get(0),
        )?;
    let mut tu = c.prepare_cached(
        "INSERT OR IGNORE INTO tex_use(tex_path_id, texset_id, slot) VALUES (?1,?2,?3)",
    )?;
    for (slot, p) in t[..n].iter().enumerate() {
        if let Some(p) = p {
            tu.execute(params![p, id, slot as i64])?;
        }
    }
    Ok(id)
}

fn tex_slots(c: &Connection, paths: &[&nif::path::TexPath]) -> rusqlite::Result<Option<i64>> {
    let mut ids = vec![];
    for p in paths {
        ids.push(if p.flags.empty || p.norm.is_empty() {
            None
        } else {
            Some(path_id(c, &p.norm)?)
        });
    }
    if ids.is_empty() {
        return Ok(None);
    }
    texset_id(c, &ids).map(Some)
}

/// Writes the shape rows of `scan` into `table` (`shape` keyed by copy id, or `pending_shape` keyed by pid).
fn write_shapes(c: &Connection, table: &str, key: i64, scan: &NifScan) -> Result<(), ScanError> {
    let sql =
        format!("INSERT OR REPLACE INTO {table} VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)");
    for s in &scan.shapes {
        let (kind, ty, f1, f2, texset, params) = match &s.shader {
            Shader::None => (0, None, None, None, None, None),
            Shader::Lighting(l, slots) => {
                let refs: Vec<&nif::path::TexPath> = slots.iter().collect();
                (
                    1,
                    Some(l.shader_type),
                    Some(l.slsf1),
                    Some(l.slsf2),
                    tex_slots(c, &refs)?,
                    Some(serde_json::to_string(&l.params).map_err(json_err)?),
                )
            }
            Shader::Effect(e) => (
                2,
                None,
                Some(e.slsf1),
                Some(e.slsf2),
                tex_slots(c, &[&e.source, &e.greyscale])?,
                Some(serde_json::to_string(&e.params).map_err(json_err)?),
            ),
            Shader::Other { kind } => (
                3,
                None,
                None,
                None,
                None,
                Some(serde_json::json!({ "shader": kind }).to_string()),
            ),
        };
        c.prepare_cached(&sql)?.execute(params![
            key,
            s.block as i64,
            s.head.name,
            format!("{:?}", s.head.kind),
            kind,
            ty.map(i64::from),
            f1.map(i64::from),
            f2.map(i64::from),
            texset,
            s.alpha.map(|a| a.flags as i64),
            s.alpha.map(|a| a.threshold as i64),
            params,
        ])?;
    }
    Ok(())
}

fn json_err(e: serde_json::Error) -> ScanError {
    ScanError::Io(std::io::Error::other(e))
}

fn upsert_copy_row(c: &Connection, it: &ChunkItem) -> Result<i64, ScanError> {
    let (status, bsver, le, problem) = it.result.record();
    let id: i64 = c
        .prepare_cached(
            "INSERT INTO nif_copy(path_id, origin_id, size, mtime, crc32, parse_status, bsver, le_mesh, is_derived, problem)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(path_id, origin_id) DO UPDATE SET size = excluded.size, mtime = excluded.mtime,
               crc32 = excluded.crc32, parse_status = excluded.parse_status, bsver = excluded.bsver,
               le_mesh = excluded.le_mesh, is_derived = excluded.is_derived, problem = excluded.problem,
               source_copy_id = NULL, derive_check = NULL
             RETURNING id",
        )?
        .query_row(
            params![
                it.path_id,
                it.origin_id,
                it.stored_size(),
                it.stored_mtime(),
                it.crc32.map(i64::from),
                status,
                bsver.map(i64::from),
                le,
                it.derived,
                problem
            ],
            |r| r.get(0),
        )?;
    c.prepare_cached("DELETE FROM shape WHERE copy_id = ?1")?
        .execute([id])?;
    if let CopyResult::Scanned(s) = &it.result {
        write_shapes(c, "shape", id, s)?;
    }
    Ok(id)
}

fn stage_derived(c: &Connection, it: &ChunkItem) -> Result<(), ScanError> {
    let (status, bsver, le, problem) = it.result.record();
    let pid: i64 = c
        .prepare_cached(
            "INSERT OR REPLACE INTO pending_derived(path_id, origin_id, size, mtime, crc32, parse_status, bsver, le_mesh, problem)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) RETURNING pid",
        )?
        .query_row(
            params![
                it.path_id,
                it.origin_id,
                it.stored_size(),
                it.stored_mtime(),
                it.crc32.map(i64::from),
                status,
                bsver.map(i64::from),
                le,
                problem
            ],
            |r| r.get(0),
        )?;
    if let CopyResult::Scanned(s) = &it.result {
        write_shapes(c, "pending_shape", pid, s)?;
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, ScanError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v == 0 {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(Store { conn })
    }

    /// Start of a scan: every origin disabled until re-upserted; staging tables created and emptied.
    pub fn begin_scan(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(TEMP)?;
        self.conn.execute("UPDATE origin SET enabled = 0", [])?;
        Ok(())
    }

    pub fn upsert_origin(&mut self, o: &OriginSpec) -> Result<i64, ScanError> {
        Ok(self.conn.query_row(
            "INSERT INTO origin(kind, name, root, parent_origin, priority, enabled, derived_by, derived_state)
             VALUES (?1,?2,?3,?4,?5,1,?6,?7)
             ON CONFLICT(kind, root) DO UPDATE SET name = excluded.name, parent_origin = excluded.parent_origin,
               priority = excluded.priority, enabled = 1, derived_by = excluded.derived_by,
               derived_state = excluded.derived_state
             RETURNING id",
            params![
                o.kind,
                o.name,
                o.root,
                o.parent,
                o.priority,
                o.derived_by,
                o.derived_state.map(|s| s as i32)
            ],
            |r| r.get(0),
        )?)
    }

    pub fn set_derived_state(&mut self, origin_id: i64, s: DerivedState) -> Result<(), ScanError> {
        self.conn.execute(
            "UPDATE origin SET derived_state = ?2 WHERE id = ?1",
            params![origin_id, s as i32],
        )?;
        Ok(())
    }

    /// Interns many paths in one transaction; returns their ids in order.
    pub fn path_ids(&mut self, paths: &[&str]) -> Result<Vec<i64>, ScanError> {
        let tx = self.conn.transaction()?;
        let mut out = Vec::with_capacity(paths.len());
        for p in paths {
            out.push(path_id(&tx, p)?);
        }
        tx.commit()?;
        Ok(out)
    }

    pub fn existing_copies(&self) -> Result<HashMap<(i64, i64), CopyMeta>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT id, path_id, origin_id, size, mtime, parse_status, le_mesh, problem, crc32 FROM nif_copy",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?),
                CopyMeta {
                    id: r.get(0)?,
                    size: r.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                    mtime: r.get::<_, Option<i64>>(4)?,
                    parse_status: r.get(5)?,
                    le_mesh: r.get(6)?,
                    problem: r.get(7)?,
                    crc32: r.get::<_, Option<i64>>(8)?.map(|c| c as u32),
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Writes one chunk in one transaction: ordinary copies into `nif_copy`/`shape`, derived ones staged.
    pub fn write_chunk(&mut self, items: &[ChunkItem]) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        for it in items {
            if it.derived {
                stage_derived(&tx, it)?;
            } else {
                upsert_copy_row(&tx, it)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves staged derived copies into `nif_copy`/`shape` (the post-walk PGPatcher check passed).
    pub fn commit_pending(&mut self) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "INSERT INTO nif_copy(path_id, origin_id, size, mtime, crc32, parse_status, bsver, le_mesh, is_derived, problem)
               SELECT path_id, origin_id, size, mtime, crc32, parse_status, bsver, le_mesh, 1, problem
               FROM pending_derived WHERE true
               ON CONFLICT(path_id, origin_id) DO UPDATE SET size = excluded.size, mtime = excluded.mtime,
                 crc32 = excluded.crc32, parse_status = excluded.parse_status, bsver = excluded.bsver,
                 le_mesh = excluded.le_mesh, is_derived = 1, problem = excluded.problem,
                 source_copy_id = NULL, derive_check = NULL;
             DELETE FROM shape WHERE copy_id IN (SELECT c.id FROM nif_copy c JOIN pending_derived d
               ON c.path_id = d.path_id AND c.origin_id = d.origin_id);
             INSERT INTO shape SELECT c.id, s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2,
               s.texset_id, s.alpha_flags, s.alpha_threshold, s.params
               FROM pending_shape s JOIN pending_derived d ON d.pid = s.pid
               JOIN nif_copy c ON c.path_id = d.path_id AND c.origin_id = d.origin_id;
             DELETE FROM pending_shape;
             DELETE FROM pending_derived;",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Tainted run: drop the staged derived copies and keep the previous rows.
    pub fn discard_pending(&mut self) -> Result<(), ScanError> {
        self.conn
            .execute_batch("DELETE FROM pending_shape; DELETE FROM pending_derived;")?;
        Ok(())
    }

    /// Writes one copy directly (outside a scan chunk) and returns its id.
    pub fn upsert_copy(
        &mut self,
        path: &str,
        origin_id: i64,
        size: u64,
        mtime: i64,
        result: CopyResult,
        crc32: Option<u32>,
    ) -> Result<i64, ScanError> {
        let tx = self.conn.transaction()?;
        let it = ChunkItem {
            path_id: path_id(&tx, path)?,
            origin_id,
            size,
            mtime,
            crc32,
            derived: false,
            result,
        };
        let id = upsert_copy_row(&tx, &it)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn replace_shapes(&mut self, copy_id: i64, scan: &NifScan) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM shape WHERE copy_id = ?1", [copy_id])?;
        write_shapes(&tx, "shape", copy_id, scan)?;
        tx.commit()?;
        Ok(())
    }

    pub fn copy_ids(&self) -> Result<HashMap<(i64, i64), i64>, ScanError> {
        let mut st = self
            .conn
            .prepare("SELECT path_id, origin_id, id FROM nif_copy")?;
        let rows = st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replaces both winner tables: `game` path_id → (copy_id, n_copies), `source` path_id → copy_id.
    pub fn set_winners(
        &mut self,
        game: &HashMap<i64, (i64, u32)>,
        source: &HashMap<i64, i64>,
    ) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM nif_winner; DELETE FROM nif_winner_src;")?;
        {
            let mut g = tx.prepare("INSERT INTO nif_winner VALUES (?1,?2,?3)")?;
            for (p, (c, n)) in game {
                g.execute(params![p, c, n])?;
            }
            let mut s = tx.prepare("INSERT INTO nif_winner_src VALUES (?1,?2)")?;
            for (p, c) in source {
                s.execute(params![p, c])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Replaces `pgp_diff` with path → (crc32original, crc32patched).
    pub fn set_pgp_diff(&mut self, diff: &HashMap<String, (u32, u32)>) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM pgp_diff", [])?;
        for (p, (o, n)) in diff {
            let id = path_id(&tx, p)?;
            tx.prepare_cached("INSERT OR REPLACE INTO pgp_diff VALUES (?1,?2,?3)")?
                .execute(params![id, *o as i64, *n as i64])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes the derived-layer columns for every copy, plus newly computed CRCs, in one transaction.
    pub fn set_derived(
        &mut self,
        rows: &[DeriveRow],
        crcs: &HashMap<i64, u32>,
    ) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        {
            let mut u = tx.prepare(
                "UPDATE nif_copy SET is_derived = ?2, source_copy_id = ?3, derive_check = ?4 WHERE id = ?1",
            )?;
            for r in rows {
                u.execute(params![
                    r.copy_id,
                    r.is_derived,
                    r.source_copy_id,
                    r.derive_check
                ])?;
            }
            let mut c = tx.prepare("UPDATE nif_copy SET crc32 = ?2 WHERE id = ?1")?;
            for (id, crc) in crcs {
                c.execute(params![id, *crc as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes copies (and their shapes) not in `seen`.
    pub fn delete_unseen(&mut self, seen: &HashSet<i64>) -> Result<usize, ScanError> {
        let all: Vec<i64> = {
            let mut st = self.conn.prepare("SELECT id FROM nif_copy")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            v
        };
        let tx = self.conn.transaction()?;
        let mut n = 0;
        for id in all.into_iter().filter(|i| !seen.contains(i)) {
            tx.prepare_cached("DELETE FROM shape WHERE copy_id = ?1")?
                .execute([id])?;
            tx.prepare_cached("DELETE FROM nif_copy WHERE id = ?1")?
                .execute([id])?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Enabled origins: loose ones and separators by priority (highest first, a separator above its group),
    /// then BSAs by game rank.
    pub fn mods(&self) -> Vec<ModRow> {
        let q = || -> rusqlite::Result<Vec<ModRow>> {
            let mut st = self.conn.prepare(
                "SELECT id, name, kind, priority, derived_by IS NOT NULL FROM origin WHERE enabled = 1
                 ORDER BY kind = 4, priority DESC, kind = 3 DESC, id",
            )?;
            let v = st
                .query_map([], |r| {
                    Ok(ModRow {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        priority: r.get(3)?,
                        derived: r.get(4)?,
                    })
                })?
                .collect();
            v
        };
        q().unwrap_or_default()
    }

    pub fn nifs_of(&self, origin_id: i64) -> Vec<NifRow> {
        let q = || -> rusqlite::Result<Vec<NifRow>> {
            let mut st = self.conn.prepare(
                "SELECT c.id, p.p, c.is_derived, c.derive_check, c.le_mesh, c.problem,
                        w.copy_id, w.n_copies, wc.is_derived, wc.source_copy_id, wc.derive_check, wo.name,
                        ws.copy_id, so.name
                 FROM nif_copy c JOIN path p ON p.id = c.path_id
                 LEFT JOIN nif_winner w ON w.path_id = c.path_id
                 LEFT JOIN nif_copy wc ON wc.id = w.copy_id
                 LEFT JOIN origin wo ON wo.id = wc.origin_id
                 LEFT JOIN nif_winner_src ws ON ws.path_id = c.path_id
                 LEFT JOIN nif_copy sc ON sc.id = ws.copy_id
                 LEFT JOIN origin so ON so.id = sc.origin_id
                 WHERE c.origin_id = ?1 ORDER BY p.p",
            )?;
            let v = st
                .query_map([origin_id], |r| {
                    let id: i64 = r.get(0)?;
                    let derived: bool = r.get(2)?;
                    let own_check: Option<i32> = r.get(3)?;
                    let win: Option<i64> = r.get(6)?;
                    let n: Option<i64> = r.get(7)?;
                    let win_derived: Option<bool> = r.get(8)?;
                    let win_source: Option<i64> = r.get(9)?;
                    let win_check: Option<i32> = r.get(10)?;
                    let win_origin: Option<String> = r.get(11)?;
                    let src: Option<i64> = r.get(12)?;
                    let src_origin: Option<String> = r.get(13)?;
                    let wins = win == Some(id);
                    let patched_in_game =
                        !wins && win_derived == Some(true) && win_source == Some(id);
                    let overridden_by = if wins {
                        None
                    } else if derived {
                        win_origin
                    } else if src.is_some() && src != Some(id) {
                        src_origin
                    } else {
                        None
                    };
                    let check = if derived {
                        own_check
                    } else if patched_in_game {
                        win_check
                    } else {
                        None
                    };
                    Ok(NifRow {
                        copy_id: id,
                        path: r.get(1)?,
                        wins,
                        overridden_by,
                        n_copies: n.unwrap_or(0) as u32,
                        derived,
                        derive_check: check.and_then(check_name).map(str::to_string),
                        le_mesh: r.get(4)?,
                        problem: r.get(5)?,
                        patched_in_game,
                    })
                })?
                .collect();
            v
        };
        q().unwrap_or_default()
    }

    pub fn shapes_of(&self, copy_id: i64) -> Vec<ShapeView> {
        let q = || -> rusqlite::Result<Vec<ShapeView>> {
            let mut st = self.conn.prepare(
                "SELECT s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2,
                        s.alpha_flags, s.alpha_threshold, s.params,
                        t.n, t.t0, t.t1, t.t2, t.t3, t.t4, t.t5, t.t6, t.t7, t.t8
                 FROM shape s LEFT JOIN texset t ON t.id = s.texset_id
                 WHERE s.copy_id = ?1 ORDER BY s.block",
            )?;
            let mut pq = self.conn.prepare("SELECT p FROM path WHERE id = ?1")?;
            let rows: Vec<(ShapeView, Vec<Option<i64>>)> = st
                .query_map([copy_id], |r| {
                    let n: Option<i64> = r.get(10)?;
                    let mut ids = vec![];
                    for i in 0..n.unwrap_or(0).clamp(0, 9) as usize {
                        ids.push(r.get::<_, Option<i64>>(11 + i)?);
                    }
                    Ok((
                        ShapeView {
                            block: r.get::<_, i64>(0)? as u32,
                            name: r.get(1)?,
                            kind: r.get(2)?,
                            shader_kind: r.get(3)?,
                            shader_type: r.get::<_, Option<i64>>(4)?.map(|x| x as u32),
                            slsf1: r.get::<_, Option<i64>>(5)?.map(|x| x as u32),
                            slsf2: r.get::<_, Option<i64>>(6)?.map(|x| x as u32),
                            textures: vec![],
                            alpha_flags: r.get::<_, Option<i64>>(7)?.map(|x| x as u16),
                            alpha_threshold: r.get::<_, Option<i64>>(8)?.map(|x| x as u8),
                            params: r.get(9)?,
                        },
                        ids,
                    ))
                })?
                .collect::<Result<_, _>>()?;
            let mut out = vec![];
            for (mut v, ids) in rows {
                for id in ids {
                    v.textures.push(match id {
                        Some(id) => pq.query_row([id], |r| r.get(0)).optional()?,
                        None => None,
                    });
                }
                out.push(v);
            }
            Ok(out)
        };
        q().unwrap_or_default()
    }

    /// Every (NIF path, slot, shader_kind) whose shapes use texture `norm`, across all copies.
    /// `shader_kind` (1 lighting, 2 effect) gives the slot its meaning: lighting slots are texture-set slots
    /// 0-8; effect slots are 0 = source, 1 = greyscale. A texset row is only a path tuple and may be shared by a
    /// lighting and an effect shape with the same paths; the meaning always comes from the using shape.
    pub fn users_of_texture(&self, norm: &str) -> Vec<(String, u32, i32)> {
        let q = || -> rusqlite::Result<Vec<(String, u32, i32)>> {
            let mut st = self.conn.prepare(
                "SELECT DISTINCT np.p, tu.slot, s.shader_kind FROM path tp
                 JOIN tex_use tu ON tu.tex_path_id = tp.id
                 JOIN shape s ON s.texset_id = tu.texset_id
                 JOIN nif_copy c ON c.id = s.copy_id
                 JOIN path np ON np.id = c.path_id
                 WHERE tp.p = ?1 ORDER BY np.p, tu.slot, s.shader_kind",
            )?;
            let v = st
                .query_map([norm], |r| {
                    Ok((r.get(0)?, r.get::<_, i64>(1)? as u32, r.get(2)?))
                })?
                .collect();
            v
        };
        q().unwrap_or_default()
    }

    /// Raw connection for read-only queries by later tasks and tests.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
