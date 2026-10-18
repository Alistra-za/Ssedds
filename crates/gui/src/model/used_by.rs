//! Item 9 (Task 12, DECISIONS AG96-AG97): the settings panel's "Used by meshes" list. One read of the scan database
//! per selected texture, on a worker thread and tagged with a generation, so a stale answer is dropped.
use super::AppModel;
use crate::worker::{spawn_guarded, WorkerMsg};
use engine::config::AlphaPolicy;
use engine::shader::{mesh_alpha_policy, MeshAlpha};
use modscan::store::{Store, KIND_BSA};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The list shows this many meshes, then "and N more".
pub const USED_BY_CAP: usize = 200;

/// One NIF copy (a mesh in a mod) whose shapes use the texture, in any slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshUser {
    pub nif: String,
    pub mod_name: String,
    pub priority: i32,
    /// From a BSA: loses to every loose copy whatever its number (archive rank).
    pub bsa: bool,
    /// Its shapes carry the alpha evidence that decided the texture's alpha meaning (AG96).
    pub decided_alpha: bool,
}

/// The answer for one texture: winner first, at most `USED_BY_CAP` rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsedBy {
    /// The item key it was read for.
    pub item: String,
    /// The scan-database path asked for (`<mesh_key>.dds`).
    pub tex: String,
    pub rows: Vec<MeshUser>,
    /// Rows past the cap.
    pub more: usize,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct UsedByState {
    /// The last answer that landed (kept on screen while a newer read runs).
    pub current: Option<Arc<UsedBy>>,
    /// A finished scan made `current` old: the next ask reads again.
    pub stale: bool,
    pub generation: u64,
    /// The item key of the read in flight.
    pub pending: Option<String>,
    /// The texture asked for last and since when: its read starts once it has stayed [`USED_BY_SETTLE`] (AG128).
    pub wanted: Option<(String, Instant)>,
    /// When a draw last asked (`used_by_for`): the wait above keeps frames coming only while something still asks.
    pub asked_at: Option<Instant>,
}

/// How long a selection must stay before its read starts, so holding Up or Down starts one read, not one per step.
pub const USED_BY_SETTLE: Duration = Duration::from_millis(150);
/// A wanted read keeps frames coming this long after the last draw asked for it.
const ASK_TTL: Duration = Duration::from_millis(500);

/// Every NIF copy of enabled origins whose shapes use `tex`, winner first (loose before BSA, then MO2 priority, the
/// order `modscan::resolve` picks winners by). `alpha`: the texture's alpha meaning came from the meshes
/// (`mesh:alpha`), so the copies whose slot-0 source shapes gave that answer are marked (AG96).
pub fn read_users(db: &Path, item: &str, tex: &str, alpha: bool) -> UsedBy {
    let mut out = UsedBy {
        item: item.into(),
        tex: tex.into(),
        ..UsedBy::default()
    };
    match query(db, tex) {
        Ok(rows) => (out.rows, out.more) = users(rows, alpha),
        Err(e) => out.error = Some(format!("Scan database: {e}")),
    }
    out
}

/// One shape row: NIF, mod, kind, priority, and whether it is a slot-0 source lighting shape with its alpha (the
/// shapes `meshes::mesh_alpha` counts).
type ShapeRow = (String, String, i32, i32, Option<MeshAlpha>);

fn query(db: &Path, tex: &str) -> Result<Vec<ShapeRow>, String> {
    let store = Store::open_read_only(db).map_err(|e| e.to_string())?;
    // shortcut: raw SQL on `Store::conn()` against modscan's schema, as `meshes::mesh_alpha`; move it into
    // `modscan::store` as a typed query when modscan is next edited.
    let mut st = store
        .conn()
        .prepare(
            "SELECT np.p, o.name, o.kind, o.priority, tu.slot, s.shader_kind, ifnull(s.slsf2, 0), c.is_derived,
                    s.alpha_flags, s.alpha_threshold
             FROM path tp
             JOIN tex_use tu ON tu.tex_path_id = tp.id
             JOIN shape s ON s.texset_id = tu.texset_id
             JOIN nif_copy c ON c.id = s.copy_id
             JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             JOIN path np ON np.id = c.path_id
             WHERE tp.p = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([tex], |r| {
            let (slot, kind, slsf2, derived): (i64, i64, i64, bool) =
                (r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?);
            // `meshes::SLOT0_SOURCE_SHAPES`: slot 0 of a lighting shape, not TruePBR (SLSF2 bit 23), not derived.
            let source = slot == 0 && kind == 1 && slsf2 & (1 << 23) == 0 && !derived;
            let flags = r.get::<_, Option<i64>>(8)?.unwrap_or(0);
            let threshold = r.get::<_, Option<i64>>(9)?.unwrap_or(0);
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                source.then(|| MeshAlpha {
                    blend: flags & 1 != 0,
                    test: flags & (1 << 9) != 0,
                    threshold: threshold.clamp(0, 255) as u8,
                }),
            ))
        })
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// One row per (NIF, mod), sorted and capped. The decided mark follows `mesh_alpha_policy`: blend comes from every
/// blending shape, the cutout threshold from the shapes testing at the largest one; "remove" singles none out.
fn users(rows: Vec<ShapeRow>, alpha: bool) -> (Vec<MeshUser>, usize) {
    let shapes: Vec<MeshAlpha> = rows.iter().filter_map(|r| r.4).collect();
    let policy = alpha.then(|| mesh_alpha_policy(&shapes)).flatten();
    let range = engine::config::ALPHA_THRESHOLD;
    let thr = |a: &MeshAlpha| Some(a.threshold.clamp(*range.start(), *range.end()));
    let decides = |a: &MeshAlpha| match policy {
        Some((AlphaPolicy::Blend, _)) => a.blend,
        Some((AlphaPolicy::Cutout, t)) => a.test && thr(a) == t,
        Some((AlphaPolicy::CutoutBlend, t)) => a.blend || (a.test && thr(a) == t),
        _ => false,
    };
    let mut by: std::collections::BTreeMap<(String, String), MeshUser> = Default::default();
    for (nif, mod_name, kind, priority, a) in rows {
        let u = by
            .entry((nif.clone(), mod_name.clone()))
            .or_insert(MeshUser {
                nif,
                mod_name,
                priority,
                bsa: kind == KIND_BSA,
                decided_alpha: false,
            });
        u.decided_alpha |= a.as_ref().is_some_and(decides);
    }
    let mut v: Vec<MeshUser> = by.into_values().collect();
    v.sort_by(|a, b| {
        (a.bsa, std::cmp::Reverse(a.priority), &a.nif, &a.mod_name).cmp(&(
            b.bsa,
            std::cmp::Reverse(b.priority),
            &b.nif,
            &b.mod_name,
        ))
    });
    let more = v.len().saturating_sub(USED_BY_CAP);
    v.truncate(USED_BY_CAP);
    (v, more)
}

impl AppModel {
    /// The "Used by meshes" list of texture `key`: `None` hides it (no scan database, or the texture has no job);
    /// `Some(None)` while the first read runs. The first ask for a texture, or after a finished scan, starts the read.
    pub fn used_by_for(&mut self, key: &str) -> Option<Option<Arc<UsedBy>>> {
        let db = self.scan_db.clone().filter(|d| self.stat_is_file(d))?;
        let j = self.item(key)?.job.as_ref()?;
        let stem = j.output.file_stem()?.to_string_lossy().into_owned();
        let tex = format!("{}.dds", engine::shader::mesh_key(&j.game_path, &stem));
        let alpha = j
            .provenance
            .get("alpha.policy")
            .is_some_and(|p| p.to_string() == "mesh:alpha");
        let st = &self.used_by;
        let have = st.current.clone().filter(|c| c.item == key);
        // Wave 2 #6: no read of a store a running or cancelled scan left empty; a list already read stays.
        if self.scan_store_unsettled {
            return have.map(Some);
        }
        let fresh = have.is_some() && !st.stale;
        if !fresh && st.pending.as_deref() != Some(key) {
            let now = Instant::now();
            let st = &mut self.used_by;
            st.asked_at = Some(now);
            if st.wanted.as_ref().map(|w| w.0.as_str()) != Some(key) {
                // A new texture: a read still in flight is for an older one and is dropped when it lands.
                st.wanted = Some((key.to_string(), now));
                st.generation += 1;
                st.pending = None;
            }
            if st
                .wanted
                .as_ref()
                .is_some_and(|w| now.duration_since(w.1) < USED_BY_SETTLE)
            {
                return Some(have);
            }
            self.used_by.pending = Some(key.to_string());
            let (generation, item) = (self.used_by.generation, key.to_string());
            spawn_guarded(self.tx.clone(), "used-by", move |tx| {
                let u = read_users(&db, &item, &tex, alpha);
                let _ = tx.send(WorkerMsg::UsedBy {
                    generation,
                    result: u,
                });
            });
        }
        Some(have)
    }

    /// A read of generation `generation` ended; an older one (another texture was asked for since) is dropped.
    pub fn used_by_landed(&mut self, generation: u64, result: UsedBy) {
        if generation != self.used_by.generation {
            return;
        }
        self.used_by.pending = None;
        self.used_by.stale = false;
        self.used_by.current = Some(Arc::new(result));
    }

    /// A read is waiting for the selection to settle or is in flight: the app keeps frames coming.
    pub fn used_by_pending(&self) -> bool {
        let st = &self.used_by;
        // Until the wanted texture is read or being read, so a settle cannot be missed; but only while a draw still
        // asks for it, so a texture nobody shows any more (another folder, a set selected) stops the frames.
        // Note AN: a locked slot's hover list being read counts too.
        st.pending.is_some()
            || !self.original.pending.is_empty()
            || st.wanted.as_ref().is_some_and(|(k, _)| {
                st.current.as_ref().is_none_or(|c| &c.item != k)
                    && st.asked_at.is_some_and(|t| t.elapsed() < ASK_TTL)
            })
    }

    /// AG128: a read that panicked shows its error in the list instead of staying "reading" for good.
    pub(super) fn used_by_panicked(&mut self, message: &str) {
        let st = &mut self.used_by;
        let Some(item) = st.pending.take() else {
            return;
        };
        st.stale = false;
        st.current = Some(Arc::new(UsedBy {
            item,
            error: Some(format!("Reading the scan database failed: {message}")),
            ..UsedBy::default()
        }));
    }

    /// A finished scan: the list is read again on the next ask (the old one stays on screen until then).
    pub(super) fn used_by_scan_changed(&mut self) {
        self.used_by.stale = true;
        self.used_by.generation += 1;
        self.used_by.pending = None;
        self.original_scan_changed();
    }
}

/// SQL condition on texture set `ts.id`: a lighting shape in a non-derived copy of an enabled origin uses it (the
/// filters of `meshes::SLOT0_SOURCE_SHAPES`, without the TruePBR one).
pub(super) const LIVE_LIGHTING_TEXSET: &str = "EXISTS (SELECT 1 FROM shape s
         JOIN nif_copy c ON c.id = s.copy_id AND c.is_derived = 0
         JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
         WHERE s.texset_id = ts.id AND s.shader_kind = 1)";

/// Note AN: the textures the scanned meshes that use `tex` as their diffuse have in slot `n` (1-based), at most
/// eight, sorted. These are what a PGPatcher lock on that slot keeps. Only lighting shapes of enabled, non-derived
/// copies count (trace-debug #42).
pub fn slot_textures(db: &Path, tex: &str, n: u8) -> Result<Vec<String>, String> {
    if !(1..=9).contains(&n) {
        return Ok(vec![]);
    }
    let store = Store::open_read_only(db).map_err(|e| e.to_string())?;
    // shortcut: raw SQL on `Store::conn()` against modscan's schema, as `query` above; move it into
    // `modscan::store` as a typed query when modscan is next edited.
    let sql = format!(
        "SELECT DISTINCT p2.p FROM path tp
         JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = 0
         JOIN texset ts ON ts.id = tu.texset_id
         JOIN path p2 ON p2.id = ts.t{}
         WHERE tp.p = ?1 AND {LIVE_LIGHTING_TEXSET} ORDER BY p2.p LIMIT 8",
        n - 1
    );
    let mut st = store.conn().prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st
        .query_map([tex], |r| r.get::<_, String>(0))
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .map_err(|e| e.to_string());
    rows
}

/// Note AN: the hover list of PGPatcher-locked slots, one worker read per (diffuse scan path, slot). A finished scan
/// clears it and drops reads in flight (their generation is old).
#[derive(Debug, Default)]
pub struct OriginalState {
    pub done: std::collections::BTreeMap<(String, u8), Arc<Vec<String>>>,
    pub pending: std::collections::BTreeSet<(String, u8)>,
    pub generation: u64,
}

impl AppModel {
    /// The scan database and the scan path of set `set`'s diffuse; `None` without either.
    fn original_source(&self, set: &str) -> Option<(std::path::PathBuf, String)> {
        let db = self.scan_db.clone().filter(|d| self.stat_is_file(d))?;
        let v = super::sets::set_views(self)
            .into_iter()
            .find(|v| engine::sets::same_id(&v.id, set))?;
        let diffuse = match v.slots.get(&super::sets::SlotKind::Diffuse)? {
            super::sets::SlotFill::Own { item } | super::sets::SlotFill::Borrowed { item, .. } => {
                item.clone()
            }
            super::sets::SlotFill::Empty => return None,
        };
        let j = self.item(&diffuse)?.job.as_ref()?;
        let stem = j.output.file_stem()?.to_string_lossy().into_owned();
        Some((
            db,
            format!("{}.dds", engine::shader::mesh_key(&j.game_path, &stem)),
        ))
    }

    /// Note AN: [`slot_textures`] of set `set`'s diffuse in slot `n`: `None` without a scan database or a diffuse
    /// with a job, `Some(None)` until [`AppModel::ask_original_textures`]'s read lands.
    pub fn original_textures(&self, set: &str, n: u8) -> Option<Option<Arc<Vec<String>>>> {
        let (_, tex) = self.original_source(set)?;
        Some(self.original.done.get(&(tex, n)).cloned())
    }

    /// Start the read of [`AppModel::original_textures`] on a worker, unless it is done or running.
    pub fn ask_original_textures(&mut self, set: &str, n: u8) {
        let Some((db, tex)) = self.original_source(set) else {
            return;
        };
        let key = (tex, n);
        let st = &mut self.original;
        if st.done.contains_key(&key) || !st.pending.insert(key.clone()) {
            return;
        }
        let generation = st.generation;
        spawn_guarded(self.tx.clone(), "original-textures", move |tx| {
            let result = slot_textures(&db, &key.0, key.1);
            let _ = tx.send(WorkerMsg::OriginalTextures {
                generation,
                key,
                result,
            });
        });
    }

    pub(super) fn original_landed(
        &mut self,
        generation: u64,
        key: (String, u8),
        result: Result<Vec<String>, String>,
    ) {
        let st = &mut self.original;
        if generation != st.generation {
            return;
        }
        st.pending.remove(&key);
        let list = result.unwrap_or_else(|e| vec![format!("Scan database: {e}")]);
        st.done.insert(key, Arc::new(list));
    }

    /// A read that panicked: its error is the answer, so a read that keeps panicking is not started every frame (a
    /// finished scan clears it, like any answer).
    pub(super) fn original_panicked(&mut self, message: &str) {
        let st = &mut self.original;
        let error = Arc::new(vec![format!("Reading the scan database failed: {message}")]);
        for key in std::mem::take(&mut st.pending) {
            st.done.insert(key, error.clone());
        }
    }

    /// A finished scan or another folder: every list is read again.
    pub(super) fn original_scan_changed(&mut self) {
        let st = &mut self.original;
        st.done.clear();
        st.pending.clear();
        st.generation += 1;
    }
}
