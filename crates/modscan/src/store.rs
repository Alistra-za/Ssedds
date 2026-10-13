//! SQLite scan store (plan 02 §6 with the PGPatcher columns of 05 §5.2). One row per NIF copy and per shape.
use crate::ScanError;
use nif::path::{PathFlags, TexPath};
use nif::scan::{NifScan, Shader};
use nif::NifError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// `PRAGMA user_version` of the current schema. Older databases are dropped and rebuilt (the store is a cache).
pub const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = r#"
CREATE TABLE path   (id INTEGER PRIMARY KEY, p TEXT NOT NULL UNIQUE);
CREATE TABLE origin (id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, name TEXT NOT NULL, root TEXT NOT NULL,
                     parent_origin INTEGER, priority INTEGER NOT NULL, enabled INTEGER NOT NULL,
                     derived_by INTEGER, derived_state INTEGER, UNIQUE(kind, root));
CREATE TABLE nif_copy (id INTEGER PRIMARY KEY, path_id INTEGER NOT NULL, origin_id INTEGER NOT NULL,
                     size INTEGER, mtime INTEGER, crc32 INTEGER, parse_status INTEGER NOT NULL,
                     bsver INTEGER, le_mesh INTEGER NOT NULL DEFAULT 0, is_derived INTEGER NOT NULL DEFAULT 0,
                     source_copy_id INTEGER, derive_check INTEGER, problem TEXT,
                     UNIQUE(path_id, origin_id));
CREATE TABLE shape (copy_id INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT, kind TEXT NOT NULL,
                     shader_kind INTEGER NOT NULL, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                     texset_id INTEGER, alpha_flags INTEGER, alpha_threshold INTEGER, params TEXT,
                     skinned INTEGER NOT NULL DEFAULT 0, vertex_desc INTEGER, num_vertices INTEGER,
                     num_triangles INTEGER,
                     PRIMARY KEY(copy_id, block)) WITHOUT ROWID;
-- Raw NIF slot strings and PathFlags bits (1 empty, 2 rebased, 4 prefixed, 8 non_dds) for every slot whose raw
-- string is not just the canonical path in another case / with backslashes, or whose flags are not default.
-- Lighting slots are texture-set slots; effect slots are 0 = source, 1 = greyscale.
CREATE TABLE tex_raw (copy_id INTEGER NOT NULL, block INTEGER NOT NULL, slot INTEGER NOT NULL, raw TEXT NOT NULL,
                     flags INTEGER NOT NULL, PRIMARY KEY(copy_id, block, slot)) WITHOUT ROWID;
CREATE TABLE texset (id INTEGER PRIMARY KEY, n INTEGER NOT NULL,
                     t0 INTEGER, t1 INTEGER, t2 INTEGER, t3 INTEGER, t4 INTEGER, t5 INTEGER, t6 INTEGER,
                     t7 INTEGER, t8 INTEGER);
CREATE TABLE tex_use (tex_path_id INTEGER, texset_id INTEGER, slot INTEGER,
                     PRIMARY KEY(tex_path_id, texset_id, slot)) WITHOUT ROWID;
CREATE TABLE nif_winner     (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL, n_copies INTEGER NOT NULL);
CREATE TABLE nif_winner_src (path_id INTEGER PRIMARY KEY, copy_id INTEGER NOT NULL);
CREATE TABLE pgp_diff (path_id INTEGER PRIMARY KEY, crc_orig INTEGER NOT NULL, crc_patched INTEGER NOT NULL);
CREATE TABLE tex_copy   (path_id INTEGER NOT NULL, origin_id INTEGER NOT NULL, size INTEGER, mtime INTEGER,
                     PRIMARY KEY(path_id, origin_id)) WITHOUT ROWID;
CREATE TABLE tex_winner (path_id INTEGER PRIMARY KEY, origin_id INTEGER NOT NULL, n_copies INTEGER NOT NULL);
CREATE INDEX nif_copy_origin ON nif_copy(origin_id);
-- The texset uniqueness key. SQLite treats NULLs as distinct in UNIQUE, so empty (NULL) slots are folded to 0
-- (path ids start at 1) and INSERT OR IGNORE dedups sets with empty slots too.
CREATE UNIQUE INDEX texset_key ON texset(n, ifnull(t0,0), ifnull(t1,0), ifnull(t2,0), ifnull(t3,0),
                     ifnull(t4,0), ifnull(t5,0), ifnull(t6,0), ifnull(t7,0), ifnull(t8,0));
CREATE INDEX shape_texset ON shape(texset_id);
-- Plugins (esp scanner spec §2.3). `local` = the FormID as the plugin stores it (master index clamped to the
-- plugin): the cache. `formid`/`txst_formid` = load-order FormIDs, set by `resolve_plugins` (NULL until then; the raw
-- local id when the master is not loaded, `rec.owner_ok` = 0). `plugin.lo_index` = load-order position, NULL when
-- not resolved for the current order (written only by `resolve_plugins`).
CREATE TABLE plugin (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, path TEXT NOT NULL,
                     size INTEGER, mtime INTEGER NOT NULL, lo_index INTEGER, light INTEGER NOT NULL,
                     flags INTEGER NOT NULL, masters_json TEXT NOT NULL, error_text TEXT);
CREATE TABLE rec (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, sig TEXT NOT NULL, edid TEXT,
                     owner_ok INTEGER NOT NULL DEFAULT 0, txst_flags INTEGER, decal INTEGER);
CREATE TABLE rec_winner (formid INTEGER PRIMARY KEY, plugin_id INTEGER NOT NULL, n_overrides INTEGER NOT NULL);
CREATE TABLE txst_slot (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, slot INTEGER NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE TABLE model_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, slot INTEGER NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE TABLE alt_tex (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, model_slot INTEGER NOT NULL,
                     shape_name TEXT NOT NULL, txst_formid INTEGER, txst_local INTEGER NOT NULL, idx INTEGER NOT NULL);
CREATE TABLE txst_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, role TEXT NOT NULL,
                     txst_formid INTEGER, txst_local INTEGER NOT NULL);
CREATE TABLE tex_ref (formid INTEGER, local INTEGER NOT NULL, plugin_id INTEGER NOT NULL, role TEXT NOT NULL,
                     path_id INTEGER NOT NULL);
CREATE INDEX rec_formid ON rec(formid);
CREATE INDEX rec_plugin ON rec(plugin_id, local);
CREATE INDEX txst_slot_path ON txst_slot(path_id);
CREATE INDEX txst_slot_plugin ON txst_slot(plugin_id, local);
CREATE INDEX model_ref_path ON model_ref(path_id);
CREATE INDEX model_ref_plugin ON model_ref(plugin_id, local);
CREATE INDEX alt_tex_txst ON alt_tex(txst_formid);
CREATE INDEX alt_tex_plugin ON alt_tex(plugin_id, local);
CREATE INDEX txst_ref_txst ON txst_ref(txst_formid);
CREATE INDEX txst_ref_plugin ON txst_ref(plugin_id, local);
CREATE INDEX tex_ref_path ON tex_ref(path_id);
CREATE INDEX tex_ref_plugin ON tex_ref(plugin_id, local);
"#;

const SHAPE_COLS: &str = "block, name, kind, shader_kind, shader_type, slsf1, slsf2, texset_id, alpha_flags, \
                          alpha_threshold, params, skinned, vertex_desc, num_vertices, num_triangles";

const TEMP: &str = r#"
CREATE TEMP TABLE IF NOT EXISTS pending_derived (pid INTEGER PRIMARY KEY, path_id INTEGER NOT NULL,
                     origin_id INTEGER NOT NULL, size INTEGER, mtime INTEGER, crc32 INTEGER,
                     parse_status INTEGER NOT NULL, bsver INTEGER, le_mesh INTEGER NOT NULL, problem TEXT,
                     UNIQUE(path_id, origin_id));
CREATE TEMP TABLE IF NOT EXISTS pending_shape (pid INTEGER NOT NULL, block INTEGER NOT NULL, name TEXT, kind TEXT NOT NULL,
                     shader_kind INTEGER NOT NULL, shader_type INTEGER, slsf1 INTEGER, slsf2 INTEGER,
                     texset_id INTEGER, alpha_flags INTEGER, alpha_threshold INTEGER, params TEXT,
                     skinned INTEGER NOT NULL DEFAULT 0, vertex_desc INTEGER, num_vertices INTEGER,
                     num_triangles INTEGER, PRIMARY KEY(pid, block));
CREATE TEMP TABLE IF NOT EXISTS pending_tex_raw (pid INTEGER NOT NULL, block INTEGER NOT NULL, slot INTEGER NOT NULL,
                     raw TEXT NOT NULL, flags INTEGER NOT NULL, PRIMARY KEY(pid, block, slot));
DELETE FROM pending_tex_raw;
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

/// Where the game gets a texture slot path from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TexResolution {
    Found { origin: String, n_copies: u32 },
    Missing,
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
    /// Lighting: texture-set slots (count read from the file, at most 9 stored). Effect: [source, greyscale].
    /// `None` = empty slot.
    pub textures: Vec<Option<String>>,
    /// Parallel to `textures`: the raw NIF string and its flags, for slots whose raw string is more than the
    /// canonical path in another case / with backslashes, or whose flags are not default. `None` otherwise.
    pub raw: Vec<Option<RawSlot>>,
    pub alpha_flags: Option<u16>,
    pub alpha_threshold: Option<u8>,
    /// `serde_json` of `LightingParams`/`EffectParams`; `{"shader":"<block type>"}` for other shaders.
    pub params: Option<String>,
    /// The shape has a skin instance.
    pub skinned: bool,
    /// BSTriShape family only (`None` for NiTriShape/NiTriStrips/particles).
    pub vertex_desc: Option<u64>,
    pub num_vertices: Option<u32>,
    pub num_triangles: Option<u32>,
}

/// A texture slot's raw NIF string (Windows-1252 decoded) and the flags of its canonicalisation.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSlot {
    pub raw: String,
    pub flags: PathFlags,
}

/// What [`Store::nifs_using_texture`] looks for in one texture-set slot (spec §2.4, 01 §1.3). `slot` is 0-based
/// (PGPatcher `slotN` − 1). Values come in PGPatcher form (backslashes, any case); the query lowercases them and turns
/// `\` into `/`, the form `path.p` stores (`canon_texture`: lower case, `/`, `textures/…`, extension kept).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TexKey {
    /// The slot path, without its extension, with a leading `textures/pbr/` read as `textures/` (and on slot 1 a
    /// trailing `_msn`/`_n` dropped), ends with `/` + `stem`, so on whole path components. `stem`: no extension, no
    /// leading `textures\`. `match_diffuse` (slot 0) and `match_normal` (slot 1).
    Suffix { slot: u32, stem: String },
    /// That same shape-side form contains `frag` (`path_contains`, slot 0).
    Contains { slot: u32, frag: String },
    /// The slot path equals `path` (with its extension; `textures\` is added when missing). `matchN`.
    Exact { slot: u32, path: String },
}

/// One Lighting shape of a game-winning NIF whose slot matches a [`TexKey`]. The SQL is a candidate filter: the
/// panel re-checks every shape with `pgpsim::shape_matches` on its raw slot strings.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchRow {
    /// The copy the game loads.
    pub copy_id: i64,
    /// Canonical mesh path (`meshes/…`, lower case, `/`).
    pub nif_path: String,
    /// The mod of the copy to load.
    pub origin: String,
    /// Block, name, skinning and LE flag in the copy to load.
    pub shape_block: u32,
    pub shape_name: Option<String>,
    pub le_mesh: bool,
    pub skinned: bool,
    /// The game winner is a PGPatcher output copy.
    pub derived: bool,
    /// The copy to read: the winner, or for a derived winner its source (`nif_winner_src`, else `source_copy_id`).
    /// The simulation applies the patch itself, so a derived copy would be patched twice (02 §5.2).
    pub load_copy_id: i64,
}

/// Slots stored in `texset` (t0..t8). A set with more is truncated and the copy gets a problem.
pub const MAX_SLOTS: usize = 9;

/// Whether `p` needs a `tex_raw` row: anything beyond "same path, other case / backslashes", or any flag except
/// a plain empty slot.
fn keep_raw(p: &TexPath) -> bool {
    let f = &p.flags;
    if f.rebased || f.prefixed || f.non_dds {
        return true;
    }
    p.raw.replace('\\', "/").to_ascii_lowercase() != p.norm
}

/// Slot, SQL condition on `tp.p`, its bound argument, and the exact post-filter.
pub(crate) type TexQuery = (u32, String, String, Box<dyn Fn(&str) -> bool>);

/// A key value in the stored form: trimmed, `/` separators, lower case, no leading `/`.
fn key_form(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .to_ascii_lowercase()
        .trim_start_matches('/')
        .to_string()
}

/// PGPatcher's shape-side form of a stored slot path (01 §1.3): extension stripped, a leading `textures/pbr/` read
/// as `textures/`, and on the normal slot a trailing `_msn`/`_n` stripped.
fn pgp_form(norm: &str, normal: bool) -> String {
    let base = match norm.rfind('.') {
        Some(i) if !norm[i..].contains('/') => &norm[..i],
        _ => norm,
    };
    let mut s = match base.strip_prefix("textures/pbr/") {
        Some(rest) => format!("textures/{rest}"),
        None => base.to_string(),
    };
    if normal {
        for suf in ["_msn", "_n"] {
            if let Some(x) = s.strip_suffix(suf) {
                s = x.to_string();
                break;
            }
        }
    }
    s
}

/// `s` for a `LIKE … ESCAPE '\'` pattern: `%` and `_` are literal (stems such as `rock01_d` contain `_`).
fn like_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(c);
    }
    o
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
            Ok(mut s) => {
                for r in &s.shapes {
                    if let Shader::Lighting(_, slots) = &r.shader {
                        if slots.len() > MAX_SLOTS {
                            s.problems.push(format!(
                                "block {}: texture set has {} slots; only the first {MAX_SLOTS} are stored",
                                r.block,
                                slots.len()
                            ));
                        }
                    }
                }
                CopyResult::Scanned(s)
            }
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
    pub(crate) conn: Connection,
    /// This scan started on an empty store (ruling R3): it runs with `synchronous = OFF`.
    cold: bool,
    /// The running scan's interning map (ruling R6). Ids stay valid only because every write failure ends the scan;
    /// a caller that retries a failed chunk must clear the map.
    pub(crate) intern: Option<Interner>,
}

pub(crate) fn path_id(c: &Connection, p: &str) -> rusqlite::Result<i64> {
    // `last_insert_rowid` is the new id only when the insert changed a row; otherwise the path already existed.
    if c.prepare_cached("INSERT OR IGNORE INTO path(p) VALUES (?1)")?
        .execute([p])?
        == 1
    {
        return Ok(c.last_insert_rowid());
    }
    c.prepare_cached("SELECT id FROM path WHERE p = ?1")?
        .query_row([p], |r| r.get(0))
}

/// Path and texset ids of the running scan (spec §2 step 5, ruling R6): loaded by `begin_scan`, dropped by `end_scan`.
/// A miss goes to SQL, so ids added by another connection stay correct.
#[derive(Default)]
pub(crate) struct Interner {
    paths: HashMap<String, i64>,
    /// `[n, t0..t8]` with empty slots as 0, as the `texset_key` index folds them.
    texsets: HashMap<[i64; MAX_SLOTS + 1], i64>,
}

pub(crate) fn intern_path(
    c: &Connection,
    i: &mut Option<Interner>,
    p: &str,
) -> rusqlite::Result<i64> {
    if let Some(id) = i.as_ref().and_then(|i| i.paths.get(p)) {
        return Ok(*id);
    }
    let id = path_id(c, p)?;
    if let Some(i) = i {
        i.paths.insert(p.to_string(), id);
    }
    Ok(id)
}

fn texset_id(
    c: &Connection,
    i: &mut Option<Interner>,
    slots: &[Option<i64>],
) -> rusqlite::Result<i64> {
    let n = slots.len().min(MAX_SLOTS);
    let mut t = [None::<i64>; MAX_SLOTS];
    t[..n].copy_from_slice(&slots[..n]);
    let mut key = [0i64; MAX_SLOTS + 1];
    key[0] = n as i64;
    for (j, x) in t.iter().enumerate() {
        key[j + 1] = x.unwrap_or(0);
    }
    if let Some(id) = i.as_ref().and_then(|i| i.texsets.get(&key)) {
        return Ok(*id);
    }
    let inserted = c
        .prepare_cached(
            "INSERT OR IGNORE INTO texset(n,t0,t1,t2,t3,t4,t5,t6,t7,t8) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?
        .execute(params![n as i64, t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8]])?;
    let id: i64 = if inserted == 1 {
        let id = c.last_insert_rowid();
        // A new set gets its tex_use rows; an existing one got them with its insert (tex_use is never deleted).
        let mut tu = c.prepare_cached(
            "INSERT OR IGNORE INTO tex_use(tex_path_id, texset_id, slot) VALUES (?1,?2,?3)",
        )?;
        for (slot, p) in t[..n].iter().enumerate() {
            if let Some(p) = p {
                tu.execute(params![p, id, slot as i64])?;
            }
        }
        id
    } else {
        let k = &key[1..];
        c.prepare_cached(
            "SELECT id FROM texset WHERE n = ?1 AND ifnull(t0,0) = ?2 AND ifnull(t1,0) = ?3 AND ifnull(t2,0) = ?4 \
             AND ifnull(t3,0) = ?5 AND ifnull(t4,0) = ?6 AND ifnull(t5,0) = ?7 AND ifnull(t6,0) = ?8 \
             AND ifnull(t7,0) = ?9 AND ifnull(t8,0) = ?10",
        )?
        .query_row(
            params![n as i64, k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], k[8]],
            |r| r.get(0),
        )?
    };
    if let Some(i) = i {
        i.texsets.insert(key, id);
    }
    Ok(id)
}

fn tex_slots(
    c: &Connection,
    i: &mut Option<Interner>,
    paths: &[&TexPath],
) -> rusqlite::Result<Option<i64>> {
    let mut ids = vec![];
    for p in paths {
        ids.push(if p.flags.empty || p.norm.is_empty() {
            None
        } else {
            Some(intern_path(c, i, &p.norm)?)
        });
    }
    if ids.is_empty() {
        return Ok(None);
    }
    texset_id(c, i, &ids).map(Some)
}

/// Shape-family tables: `shape`/`tex_raw` keyed by copy id, or `pending_shape`/`pending_tex_raw` keyed by pid.
#[derive(Clone, Copy)]
struct Tables {
    shape: &'static str,
    raw: &'static str,
    key: &'static str,
}
const LIVE: Tables = Tables {
    shape: "shape",
    raw: "tex_raw",
    key: "copy_id",
};
const PENDING: Tables = Tables {
    shape: "pending_shape",
    raw: "pending_tex_raw",
    key: "pid",
};

/// Writes the shape rows (and raw slot rows) of `scan` under `key`.
fn write_shapes(
    c: &Connection,
    i: &mut Option<Interner>,
    t: Tables,
    key: i64,
    scan: &NifScan,
) -> Result<(), ScanError> {
    let sql = format!(
        "INSERT OR REPLACE INTO {}({}, {SHAPE_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        t.shape, t.key
    );
    let raw_sql = format!(
        "INSERT OR REPLACE INTO {}({}, block, slot, raw, flags) VALUES (?1,?2,?3,?4,?5)",
        t.raw, t.key
    );
    for s in &scan.shapes {
        let mut slots: Vec<&TexPath> = vec![];
        let (kind, ty, f1, f2, texset, params) = match &s.shader {
            Shader::None => (0, None, None, None, None, None),
            Shader::Lighting(l, ts) => {
                slots = ts.iter().take(MAX_SLOTS).collect();
                (
                    1,
                    Some(l.shader_type),
                    Some(l.slsf1),
                    Some(l.slsf2),
                    tex_slots(c, i, &slots)?,
                    Some(serde_json::to_string(&l.params).map_err(json_err)?),
                )
            }
            Shader::Effect(e) => {
                slots = vec![&e.source, &e.greyscale];
                (
                    2,
                    None,
                    Some(e.slsf1),
                    Some(e.slsf2),
                    tex_slots(c, i, &slots)?,
                    Some(serde_json::to_string(&e.params).map_err(json_err)?),
                )
            }
            Shader::Other { kind } => (
                3,
                None,
                None,
                None,
                None,
                Some(serde_json::json!({ "shader": kind }).to_string()),
            ),
        };
        let h = &s.head;
        c.prepare_cached(&sql)?.execute(params![
            key,
            s.block as i64,
            h.name,
            format!("{:?}", h.kind),
            kind,
            ty.map(i64::from),
            f1.map(i64::from),
            f2.map(i64::from),
            texset,
            s.alpha.map(|a| a.flags as i64),
            s.alpha.map(|a| a.threshold as i64),
            params,
            h.skinned,
            h.vertex_desc.map(|v| v as i64),
            h.num_vertices.map(i64::from),
            h.num_triangles.map(i64::from),
        ])?;
        for (slot, p) in slots.iter().enumerate() {
            if keep_raw(p) {
                c.prepare_cached(&raw_sql)?.execute(params![
                    key,
                    s.block as i64,
                    slot as i64,
                    p.raw,
                    p.flags.bits() as i64
                ])?;
            }
        }
    }
    Ok(())
}

fn delete_shapes(c: &Connection, copy_id: i64) -> rusqlite::Result<()> {
    c.prepare_cached("DELETE FROM shape WHERE copy_id = ?1")?
        .execute([copy_id])?;
    c.prepare_cached("DELETE FROM tex_raw WHERE copy_id = ?1")?
        .execute([copy_id])?;
    Ok(())
}

fn json_err(e: serde_json::Error) -> ScanError {
    ScanError::Io(std::io::Error::other(e))
}

fn upsert_copy_row(
    c: &Connection,
    i: &mut Option<Interner>,
    it: &ChunkItem,
) -> Result<i64, ScanError> {
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
    delete_shapes(c, id)?;
    if let CopyResult::Scanned(s) = &it.result {
        write_shapes(c, i, LIVE, id, s)?;
    }
    Ok(id)
}

fn stage_derived(
    c: &Connection,
    i: &mut Option<Interner>,
    it: &ChunkItem,
) -> Result<(), ScanError> {
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
        write_shapes(c, i, PENDING, pid, s)?;
    }
    Ok(())
}

/// The slot, the SQL condition on `eff` (the stored slot string as an SQL expression, argument `?2`), its argument
/// and the exact post-filter for `key` (spec §2.4 of the PGPatcher plan); `None` when the key is empty (no match).
pub(crate) fn tex_query(key: &TexKey, eff: &str) -> Option<TexQuery> {
    // `pgp_form` minus the extension and `_n`/`_msn` cuts, which only shorten the tail: a superset prefilter.
    let pgp = format!(
        "(CASE WHEN {eff} LIKE 'textures/pbr/%' THEN 'textures/' || substr({eff}, 14) ELSE {eff} END)"
    );
    Some(match key {
        TexKey::Suffix { slot, stem } => {
            let stem = key_form(stem);
            if stem.is_empty() {
                return None;
            }
            let pattern = format!("%/{}%", like_escape(&stem));
            let want = format!("/{stem}");
            let normal = *slot == 1;
            (
                *slot,
                format!("{pgp} LIKE ?2 ESCAPE '\\'"),
                pattern,
                Box::new(move |p: &str| pgp_form(p, normal).ends_with(&want)),
            )
        }
        TexKey::Contains { slot, frag } => {
            let frag = key_form(frag);
            if frag.is_empty() {
                return None;
            }
            let pattern = format!("%{}%", like_escape(&frag));
            let normal = *slot == 1;
            (
                *slot,
                format!("{pgp} LIKE ?2 ESCAPE '\\'"),
                pattern,
                Box::new(move |p: &str| pgp_form(p, normal).contains(&frag)),
            )
        }
        TexKey::Exact { slot, path } => {
            let p = key_form(path);
            let p = if p.starts_with("textures/") {
                p
            } else {
                format!("textures/{p}")
            };
            (*slot, format!("{eff} = ?2"), p, Box::new(|_: &str| true))
        }
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, ScanError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        match Self::open_rw(path) {
            // Ruling R4: the store is a cache; a file SQLite cannot read (power loss during synchronous = OFF) is rebuilt.
            Err(ScanError::Db(e))
                if matches!(
                    e.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
                ) =>
            {
                for ext in ["", "-wal", "-shm"] {
                    match std::fs::remove_file(format!("{}{ext}", path.display())) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                        _ => {}
                    }
                }
                Self::open_rw(path)
            }
            r => r,
        }
    }

    fn open_rw(path: &Path) -> Result<Store, ScanError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Spec §2 step 6: temp tables in memory, a 64 MB page cache, 256 MB of memory-mapped reads.
        conn.execute_batch(
            "PRAGMA temp_store = MEMORY; PRAGMA cache_size = -65536; PRAGMA mmap_size = 268435456;",
        )?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v != SCHEMA_VERSION {
            // The store is a cache: any other version is dropped and rebuilt.
            let tx = conn.unchecked_transaction()?;
            let tables: Vec<String> = {
                let mut st = tx.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                )?;
                let v = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                v
            };
            for t in tables {
                tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{t}\""))?;
            }
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Store {
            conn,
            cold: false,
            intern: None,
        })
    }

    /// A connection that never writes database content: no folder or file is created, no WAL switch, and another schema version is
    /// [`ScanError::SchemaVersion`] instead of a rebuild (a scan may be writing the database right now). SQLite may still create `-wal`/`-shm` sidecar files next to a
    /// WAL-mode database on a read-only open; the database content is never written.
    pub fn open_read_only(path: &Path) -> Result<Store, ScanError> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if found != SCHEMA_VERSION {
            return Err(ScanError::SchemaVersion {
                found,
                want: SCHEMA_VERSION,
            });
        }
        Ok(Store {
            conn,
            cold: false,
            intern: None,
        })
    }

    /// Start of a scan: staging tables created and emptied; every origin disabled until this run commits.
    /// `origin.enabled = 1` therefore means "part of the latest completed run" and is the filter of every read query.
    /// `scan::run` stages its origins with `upsert_origin_staged` and enables them with `enable_origins` at the end,
    /// so a cancelled or failed run leaves the read lists empty instead of mixing in the previous run's winners.
    /// Rows of disabled origins stay cached (a disabled mod or a Folder scan in between costs no re-parse).
    pub fn begin_scan(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(TEMP)?;
        self.conn.execute("UPDATE origin SET enabled = 0", [])?;
        self.cold = !self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM nif_copy)", [], |r| {
                r.get::<_, bool>(0)
            })?;
        if self.cold {
            // Ruling R4: a cold store has nothing to lose to a crash; a power loss is caught by `open`.
            self.conn.pragma_update(None, "synchronous", "OFF")?;
        }
        let mut i = Interner::default();
        {
            let mut st = self.conn.prepare("SELECT p, id FROM path")?;
            for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (p, id) = r?;
                i.paths.insert(p, id);
            }
            let mut st = self.conn.prepare(
                "SELECT id, n, ifnull(t0,0), ifnull(t1,0), ifnull(t2,0), ifnull(t3,0), ifnull(t4,0), ifnull(t5,0), \
                 ifnull(t6,0), ifnull(t7,0), ifnull(t8,0) FROM texset",
            )?;
            for r in st.query_map([], |r| {
                let mut k = [0i64; MAX_SLOTS + 1];
                for (j, v) in k.iter_mut().enumerate() {
                    *v = r.get(j + 1)?;
                }
                Ok((k, r.get::<_, i64>(0)?))
            })? {
                let (k, id) = r?;
                i.texsets.insert(k, id);
            }
        }
        self.intern = Some(i);
        Ok(())
    }

    /// End of every run, whatever its outcome: resets the per-scan `cold` flag.
    pub fn end_scan(&mut self) -> Result<(), ScanError> {
        self.cold = false;
        self.intern = None;
        self.conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }

    /// Upserts an origin and enables it at once.
    pub fn upsert_origin(&mut self, o: &OriginSpec) -> Result<i64, ScanError> {
        self.upsert_origin_as(o, 1)
    }

    /// Upserts an origin left disabled: part of the run in progress, not yet of the readable data.
    pub fn upsert_origin_staged(&mut self, o: &OriginSpec) -> Result<i64, ScanError> {
        self.upsert_origin_as(o, 0)
    }

    /// The commit point of a scan: the given (staged) origins become the readable run.
    pub fn enable_origins(&mut self, ids: &[i64]) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare("UPDATE origin SET enabled = 1 WHERE id = ?1")?;
            for id in ids {
                st.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn upsert_origin_as(&mut self, o: &OriginSpec, enabled: i32) -> Result<i64, ScanError> {
        Ok(self.conn.query_row(
            "INSERT INTO origin(kind, name, root, parent_origin, priority, enabled, derived_by, derived_state)
             VALUES (?1,?2,?3,?4,?5,?8,?6,?7)
             ON CONFLICT(kind, root) DO UPDATE SET name = excluded.name, parent_origin = excluded.parent_origin,
               priority = excluded.priority, enabled = excluded.enabled, derived_by = excluded.derived_by,
               derived_state = excluded.derived_state
             RETURNING id",
            params![
                o.kind,
                o.name,
                o.root,
                o.parent,
                o.priority,
                o.derived_by,
                o.derived_state.map(|s| s as i32),
                enabled
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
            out.push(intern_path(&tx, &mut self.intern, p)?);
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
                stage_derived(&tx, &mut self.intern, it)?;
            } else {
                upsert_copy_row(&tx, &mut self.intern, it)?;
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
             DELETE FROM tex_raw WHERE copy_id IN (SELECT c.id FROM nif_copy c JOIN pending_derived d
               ON c.path_id = d.path_id AND c.origin_id = d.origin_id);
             INSERT INTO shape(copy_id, block, name, kind, shader_kind, shader_type, slsf1, slsf2, texset_id,
                 alpha_flags, alpha_threshold, params, skinned, vertex_desc, num_vertices, num_triangles)
               SELECT c.id, s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2, s.texset_id,
                 s.alpha_flags, s.alpha_threshold, s.params, s.skinned, s.vertex_desc, s.num_vertices, s.num_triangles
               FROM pending_shape s JOIN pending_derived d ON d.pid = s.pid
               JOIN nif_copy c ON c.path_id = d.path_id AND c.origin_id = d.origin_id;
             INSERT INTO tex_raw(copy_id, block, slot, raw, flags)
               SELECT c.id, r.block, r.slot, r.raw, r.flags
               FROM pending_tex_raw r JOIN pending_derived d ON d.pid = r.pid
               JOIN nif_copy c ON c.path_id = d.path_id AND c.origin_id = d.origin_id;
             DELETE FROM pending_tex_raw;
             DELETE FROM pending_shape;
             DELETE FROM pending_derived;",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Tainted run: drop the staged derived copies and keep the previous rows.
    pub fn discard_pending(&mut self) -> Result<(), ScanError> {
        self.conn.execute_batch(
            "DELETE FROM pending_tex_raw; DELETE FROM pending_shape; DELETE FROM pending_derived;",
        )?;
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
            path_id: intern_path(&tx, &mut self.intern, path)?,
            origin_id,
            size,
            mtime,
            crc32,
            derived: false,
            result,
        };
        let id = upsert_copy_row(&tx, &mut self.intern, &it)?;
        tx.commit()?;
        Ok(id)
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

    /// Replaces `tex_copy` and `tex_winner`: copies as (path_id, origin_id, size, mtime), winners
    /// path_id -> (origin_id, n_copies).
    pub fn set_textures(
        &mut self,
        copies: &[(i64, i64, u64, i64)],
        winners: &HashMap<i64, (i64, u32)>,
    ) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM tex_copy; DELETE FROM tex_winner;")?;
        {
            let mut c = tx.prepare("INSERT OR REPLACE INTO tex_copy VALUES (?1,?2,?3,?4)")?;
            for (p, o, s, m) in copies {
                c.execute(params![p, o, *s as i64, m])?;
            }
            let mut w = tx.prepare("INSERT INTO tex_winner VALUES (?1,?2,?3)")?;
            for (p, (o, n)) in winners {
                w.execute(params![p, o, n])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The origin supplying the texture the game loads for `norm` (canonical path), or Missing.
    pub fn resolve_texture(&self, norm: &str) -> Result<TexResolution, ScanError> {
        Ok(self
            .conn
            .query_row(
                "SELECT o.name, w.n_copies FROM tex_winner w JOIN path p ON p.id = w.path_id
                 JOIN origin o ON o.id = w.origin_id WHERE p.p = ?1 AND o.enabled = 1",
                [norm],
                |r| {
                    Ok(TexResolution::Found {
                        origin: r.get(0)?,
                        n_copies: r.get(1)?,
                    })
                },
            )
            .optional()?
            .unwrap_or(TexResolution::Missing))
    }

    /// Where the game's copy of texture `norm` lives: (origin kind, origin root). A BSA origin's root is the
    /// archive file; any other root is a folder holding `norm` as a loose file.
    pub fn texture_location(&self, norm: &str) -> Result<Option<(i32, String)>, ScanError> {
        Ok(self
            .conn
            .query_row(
                "SELECT o.kind, o.root FROM tex_winner w JOIN path p ON p.id = w.path_id
                 JOIN origin o ON o.id = w.origin_id WHERE p.p = ?1 AND o.enabled = 1",
                [norm],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// A NIF copy's (canonical path, origin kind, origin root, LE mesh), for reading its bytes (see
    /// [`Store::texture_location`] for the root).
    pub fn copy_location(
        &self,
        copy_id: i64,
    ) -> Result<Option<(String, i32, String, bool)>, ScanError> {
        Ok(self
            .conn
            .query_row(
                "SELECT p.p, o.kind, o.root, c.le_mesh FROM nif_copy c JOIN path p ON p.id = c.path_id
                 JOIN origin o ON o.id = c.origin_id WHERE c.id = ?1",
                [copy_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
    }

    /// Replaces `pgp_diff` with path → (crc32original, crc32patched).
    pub fn set_pgp_diff(&mut self, diff: &HashMap<String, (u32, u32)>) -> Result<(), ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM pgp_diff", [])?;
        for (p, (o, n)) in diff {
            let id = intern_path(&tx, &mut self.intern, p)?;
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

    /// Deletes the copies (with their shape and raw rows) of origins enabled in this run that are not in `seen`.
    /// Copies of origins outside this run (disabled mods, another source) are kept as cache.
    pub fn delete_unseen(&mut self, seen: &HashSet<i64>) -> Result<usize, ScanError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS seen_copy (id INTEGER PRIMARY KEY); DELETE FROM seen_copy;
             CREATE TEMP TABLE IF NOT EXISTS gone_copy (id INTEGER PRIMARY KEY); DELETE FROM gone_copy;",
        )?;
        {
            let mut ins = tx.prepare("INSERT OR IGNORE INTO seen_copy VALUES (?1)")?;
            for id in seen {
                ins.execute([id])?;
            }
        }
        let n = tx.execute(
            "INSERT INTO gone_copy SELECT c.id FROM nif_copy c JOIN origin o ON o.id = c.origin_id
             WHERE o.enabled = 1 AND c.id NOT IN (SELECT id FROM seen_copy)",
            [],
        )?;
        tx.execute_batch(
            "DELETE FROM shape WHERE copy_id IN (SELECT id FROM gone_copy);
             DELETE FROM tex_raw WHERE copy_id IN (SELECT id FROM gone_copy);
             DELETE FROM nif_copy WHERE id IN (SELECT id FROM gone_copy);
             DELETE FROM seen_copy; DELETE FROM gone_copy;",
        )?;
        tx.commit()?;
        Ok(n)
    }

    /// Origins of the latest run: loose ones and separators by priority (highest first), then BSAs by game rank.
    /// A separator is listed after the mods it owns: MO2 shows a separator above its mods in ascending priority,
    /// so in this highest-first order it owns the loose mods listed just above it, up to the previous separator.
    pub fn mods(&self) -> Result<Vec<ModRow>, ScanError> {
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
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// NIF copies of `origin_id`, by path. Empty when the origin is not part of the latest run.
    pub fn nifs_of(&self, origin_id: i64) -> Result<Vec<NifRow>, ScanError> {
        let mut st = self
            .conn
            .prepare(&format!("{NIF_ROWS} WHERE c.origin_id = ?1 ORDER BY p.p"))?;
        let v = st
            .query_map([origin_id], |r| nif_row(r).map(|(n, _)| n))?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// The game-winning copy of each mesh path that contains `path_contains` (the part after a leading `meshes/`; case-insensitive, `\` read as `/`,
    /// no wildcards), by path, with its origin's name. Returns up to `limit + 1` rows: one extra means "truncated".
    pub fn winning_nifs(
        &self,
        path_contains: &str,
        limit: usize,
    ) -> Result<Vec<(NifRow, String)>, ScanError> {
        let pattern = format!(
            "%{}%",
            like_escape(&path_contains.replace('\\', "/").to_lowercase())
        );
        let mut st = self.conn.prepare(&format!(
            "{NIF_ROWS} WHERE w.copy_id = c.id AND CASE WHEN p.p LIKE 'meshes/%' THEN substr(p.p, 8) ELSE p.p END LIKE ?1 ESCAPE '\\' ORDER BY p.p LIMIT ?2"
        ))?;
        let v = st
            .query_map(rusqlite::params![pattern, limit as i64 + 1], nif_row)?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// Shape rows of one copy, by block, with slot paths, raw slot strings and geometry facts.
    pub fn shapes_of(&self, copy_id: i64) -> Result<Vec<ShapeView>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2,
                    s.alpha_flags, s.alpha_threshold, s.params,
                    s.skinned, s.vertex_desc, s.num_vertices, s.num_triangles,
                    t.n, t.t0, t.t1, t.t2, t.t3, t.t4, t.t5, t.t6, t.t7, t.t8
             FROM shape s LEFT JOIN texset t ON t.id = s.texset_id
             WHERE s.copy_id = ?1 ORDER BY s.block",
        )?;
        const T: usize = 14;
        let rows: Vec<(ShapeView, Vec<Option<i64>>)> = st
            .query_map([copy_id], |r| {
                let n: Option<i64> = r.get(T)?;
                let mut ids = vec![];
                for i in 0..n.unwrap_or(0).clamp(0, MAX_SLOTS as i64) as usize {
                    ids.push(r.get::<_, Option<i64>>(T + 1 + i)?);
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
                        raw: vec![],
                        alpha_flags: r.get::<_, Option<i64>>(7)?.map(|x| x as u16),
                        alpha_threshold: r.get::<_, Option<i64>>(8)?.map(|x| x as u8),
                        params: r.get(9)?,
                        skinned: r.get(10)?,
                        vertex_desc: r.get::<_, Option<i64>>(11)?.map(|x| x as u64),
                        num_vertices: r.get::<_, Option<i64>>(12)?.map(|x| x as u32),
                        num_triangles: r.get::<_, Option<i64>>(13)?.map(|x| x as u32),
                    },
                    ids,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut raws: HashMap<(u32, usize), RawSlot> = HashMap::new();
        {
            let mut rq = self
                .conn
                .prepare("SELECT block, slot, raw, flags FROM tex_raw WHERE copy_id = ?1")?;
            let it = rq.query_map([copy_id], |r| {
                Ok((
                    (r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as usize),
                    RawSlot {
                        raw: r.get(2)?,
                        flags: PathFlags::from_bits(r.get::<_, i64>(3)? as u8),
                    },
                ))
            })?;
            for x in it {
                let (k, v) = x?;
                raws.insert(k, v);
            }
        }
        let mut pq = self.conn.prepare("SELECT p FROM path WHERE id = ?1")?;
        let mut out = vec![];
        for (mut v, ids) in rows {
            for (slot, id) in ids.into_iter().enumerate() {
                v.textures.push(match id {
                    Some(id) => pq.query_row([id], |r| r.get(0)).optional()?,
                    None => None,
                });
                v.raw.push(raws.remove(&(v.block, slot)));
            }
            out.push(v);
        }
        Ok(out)
    }

    /// Every (NIF path, slot, shader_kind) whose shapes use texture `norm`, across the copies of the latest run's
    /// origins. `shader_kind` (1 lighting, 2 effect) gives the slot its meaning: lighting slots are texture-set
    /// slots 0-8; effect slots are 0 = source, 1 = greyscale. A texset row is only a path tuple and may be shared
    /// by a lighting and an effect shape with the same paths; the meaning always comes from the using shape.
    pub fn users_of_texture(&self, norm: &str) -> Result<Vec<(String, u32, i32)>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT DISTINCT np.p, tu.slot, s.shader_kind FROM path tp
             JOIN tex_use tu ON tu.tex_path_id = tp.id
             JOIN shape s ON s.texset_id = tu.texset_id
             JOIN nif_copy c ON c.id = s.copy_id
             JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             JOIN path np ON np.id = c.path_id
             WHERE tp.p = ?1 ORDER BY np.p, tu.slot, s.shader_kind",
        )?;
        let v = st
            .query_map([norm], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)? as u32, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }

    /// Lighting shapes of game-winning NIFs whose slot matches `key` (spec §2.4), best first: non-skinned, fewest
    /// shapes in the NIF, then path and block. At most `limit` rows. A derived winner is matched on its source
    /// copy's shapes. `Suffix`/`Contains` cannot use the path index (a scan of `path`); the caller runs this off the
    /// UI thread.
    pub fn nifs_using_texture(
        &self,
        key: &TexKey,
        limit: usize,
    ) -> Result<Vec<MatchRow>, ScanError> {
        if limit == 0 {
            return Ok(vec![]);
        }
        // The slot string PGPatcher sees: the raw one when `tex_raw` keeps it, else the canonical path; lower case, `/`.
        let eff = "lower(replace(coalesce(tr.raw, tp.p), char(92), '/'))";
        let Some((slot, cond, arg, check)) = tex_query(key, eff) else {
            return Ok(vec![]);
        };

        let sql = format!(
            "SELECT wc.id, np.p, co.name, s.block, s.name, c.le_mesh, s.skinned, wc.is_derived, c.id, {eff},
                    (SELECT count(*) FROM shape s2 WHERE s2.copy_id = c.id) AS n_shapes
             FROM path tp
             JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = ?1
             JOIN shape s ON s.texset_id = tu.texset_id AND s.shader_kind = 1
             JOIN nif_copy c ON c.id = s.copy_id
             LEFT JOIN tex_raw tr ON tr.copy_id = c.id AND tr.block = s.block AND tr.slot = ?1
             JOIN origin co ON co.id = c.origin_id AND co.enabled = 1
             JOIN nif_winner w ON w.path_id = c.path_id
             JOIN nif_copy wc ON wc.id = w.copy_id
             JOIN origin wo ON wo.id = wc.origin_id AND wo.enabled = 1
             LEFT JOIN nif_winner_src ws ON ws.path_id = c.path_id
             JOIN path np ON np.id = c.path_id
             WHERE {cond}
               AND c.id = CASE WHEN wc.is_derived = 1 THEN coalesce(ws.copy_id, wc.source_copy_id, wc.id)
                               ELSE wc.id END
             ORDER BY s.skinned, n_shapes, np.p, s.block"
        );
        let mut st = self.conn.prepare(&sql)?;
        let mut rows = st.query(params![slot as i64, arg])?;
        let mut out = vec![];
        while let Some(r) = rows.next()? {
            let tex: String = r.get(9)?;
            if !check(&tex) {
                continue;
            }
            out.push(MatchRow {
                copy_id: r.get(0)?,
                nif_path: r.get(1)?,
                origin: r.get(2)?,
                shape_block: r.get::<_, i64>(3)? as u32,
                shape_name: r.get(4)?,
                le_mesh: r.get(5)?,
                skinned: r.get(6)?,
                derived: r.get(7)?,
                load_copy_id: r.get(8)?,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Raw connection for read-only queries by later tasks and tests.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Columns and joins shared by `nifs_of` and `winning_nifs`; each adds its own WHERE and ORDER BY.
const NIF_ROWS: &str = "SELECT c.id, p.p, c.is_derived, c.derive_check, c.le_mesh, c.problem,
        w.copy_id, w.n_copies, wc.is_derived, wc.source_copy_id, wc.derive_check, wo.name,
        ws.copy_id, so.name, co.name
 FROM nif_copy c JOIN path p ON p.id = c.path_id
 JOIN origin co ON co.id = c.origin_id AND co.enabled = 1
 LEFT JOIN nif_winner w ON w.path_id = c.path_id
 LEFT JOIN nif_copy wc ON wc.id = w.copy_id
 LEFT JOIN origin wo ON wo.id = wc.origin_id
 LEFT JOIN nif_winner_src ws ON ws.path_id = c.path_id
 LEFT JOIN nif_copy sc ON sc.id = ws.copy_id
 LEFT JOIN origin so ON so.id = sc.origin_id";

/// One `NIF_ROWS` row and the name of the origin that holds the copy.
fn nif_row(r: &rusqlite::Row) -> rusqlite::Result<(NifRow, String)> {
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
    let patched_in_game = !wins && win_derived == Some(true) && win_source == Some(id);
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
    let row = NifRow {
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
    };
    Ok((row, r.get(14)?))
}
