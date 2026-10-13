//! Scan orchestrator (plan 02 §2/§6, 05 §5): walk origins, parse NIFs, store shapes, resolve game and source
//! winners, link PGPatcher-derived copies to their sources.
use crate::archives::{active_bsas, ActiveBsa};
use crate::mo2::{loose_origins, Instance, LooseOrigin, OriginKind};
use crate::pgp::{self, DeriveCheck, Diff, Signature};
use crate::resolve::{winners, Copy, OriginRef};
use crate::store::{
    check_code, ChunkItem, CopyResult, DeriveRow, DerivedState, OriginSpec, Store, KIND_BSA,
    KIND_DATA, KIND_MOD, KIND_OVERWRITE, KIND_SEPARATOR, PARSE_FAILED, PARSE_NOT_SKYRIM,
};
use crate::ScanError;
use nif::path::is_facegen;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use vfs::bsa::Bsa;

const CHUNK: usize = 512;

/// Note of a run cancelled during the plugin pass (ruling R13): the NIF results are complete.
pub const PLUGINS_CANCELLED: &str =
    "Plugin scan cancelled: plugin results are incomplete until the next scan.";

#[derive(Debug, Clone)]
pub enum Source {
    Mo2(Instance),
    Folder(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
}

/// An origin carrying the PGPatcher signature (05 §5.1).
#[derive(Debug, Clone, PartialEq)]
pub struct PgpOrigin {
    pub origin_id: i64,
    pub name: String,
    pub signature: Signature,
    pub state: DerivedState,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    /// NIF copies seen in this walk (FaceGen excluded).
    pub nifs: usize,
    pub parsed: usize,
    pub reused: usize,
    /// (location, message): parse failures (status 3) of all seen copies, plus unreadable BSAs.
    pub problems: Vec<(String, String)>,
    pub not_skyrim: usize,
    pub le_meshes: usize,
    pub pgp: Vec<PgpOrigin>,
    pub tainted: bool,
    pub missing_mods: Vec<String>,
    /// Informational notes (e.g. no INI archive list found).
    pub notes: Vec<String>,
    /// Wall time per phase in milliseconds, in run order: origins, walk, parse, write, resolve, textures, plugins.
    pub phase_ms: Vec<(&'static str, u64)>,
    /// Sub-timers in milliseconds (spec §2 step 1; ruling R2). `*_cpu` entries are summed over the parse threads.
    pub sub_ms: Vec<(&'static str, u64)>,
    /// The plugin pass (esp spec §2.2): `Some` after an MO2 run whose plugin pass finished.
    pub plugins: Option<crate::plugins::PluginStats>,
}

/// Elapsed milliseconds since `t`, and restarts `t`.
fn lap(t: &mut Instant) -> u64 {
    let ms = t.elapsed().as_millis() as u64;
    *t = Instant::now();
    ms
}

/// Nanoseconds since `t` (sub-timers summed across threads).
fn ns(t: Instant) -> u64 {
    t.elapsed().as_nanos() as u64
}

/// Scans one file's bytes; a parser panic becomes a `Failed` copy instead of aborting the scan.
pub fn scan_guarded(b: &[u8]) -> CopyResult {
    guarded(|| nif::scan::scan(b))
}

fn guarded(f: impl FnOnce() -> Result<nif::scan::NifScan, nif::NifError>) -> CopyResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => CopyResult::from(r),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            CopyResult::Failed(format!("parser panic: {msg}"))
        }
    }
}

enum Loc {
    Loose(PathBuf),
    Bsa(usize, usize),
}

pub(crate) fn file_mtime(p: &std::path::Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// `_separator` lines from modlist.txt (`+` or `-`; MO2 writes them as `-`), each with the priority of the
/// first enabled mod below it (-1 if none).
fn separators(inst: &Instance, loose: &[LooseOrigin]) -> Result<Vec<(String, i32)>, ScanError> {
    let text = std::fs::read_to_string(inst.profiles.join(&inst.profile).join("modlist.txt"))?;
    let prio: HashMap<&str, i32> = loose
        .iter()
        .filter(|o| o.kind == OriginKind::Mod)
        .map(|o| (o.name.as_str(), o.priority))
        .collect();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        let Some(name) = l.strip_prefix('+').or_else(|| l.strip_prefix('-')) else {
            continue;
        };
        let name = name.trim();
        if !name.ends_with("_separator") {
            continue;
        }
        let below = lines[i + 1..]
            .iter()
            .filter_map(|l| l.strip_prefix('+'))
            .find_map(|n| prio.get(n.trim()).copied())
            .unwrap_or(-1);
        out.push((name.to_string(), below));
    }
    Ok(out)
}

fn kind_code(k: OriginKind) -> i32 {
    match k {
        OriginKind::GameData => KIND_DATA,
        OriginKind::Mod => KIND_MOD,
        OriginKind::Overwrite => KIND_OVERWRITE,
    }
}

pub fn run(
    src: &Source,
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
    procs: &dyn Fn() -> Vec<String>,
) -> Result<Report, ScanError> {
    run_with(src, store, cancel, progress, procs, false)
}

/// `run`, optionally without the PGPatcher output ("Scan without PGPatcher output", Task 12 / ruling R6). With
/// `skip_derived`, a running PGPatcher does not block or taint the scan, mods carrying the PGPatcher signature are
/// left out of the run (as if disabled), and in other origins every derived path (dups, diff-listed) is skipped.
pub fn run_with(
    src: &Source,
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
    procs: &dyn Fn() -> Vec<String>,
    skip_derived: bool,
) -> Result<Report, ScanError> {
    // Declared first so it drops last, after end_scan, on every path including a panic.
    let _lock = ScanLock::take(store)?;
    let r = scan_body(src, store, cancel, progress, procs, skip_derived);
    if let Err(e) = &r {
        // Pending rows must not outlive a failed run (first error wins). A crash after a cold direct write keeps
        // the derived copies; their size/mtime re-parse guards them.
        let _ = store.discard_pending();
        reset_if_corrupt(store, e);
    }
    // Every outcome, cancel and errors included, ends the scan (resets the store's per-scan state).
    let t = Instant::now();
    let end = store.end_scan();
    let mut report = r?;
    end?;
    report
        .sub_ms
        .push(("end_scan", t.elapsed().as_millis() as u64));
    Ok(report)
}

/// Exclusive `scan.lock` beside the store file for the whole run (audit 17: a second scan's `delete_unseen` would
/// wipe the first one's rows). Windows: the file is held open with share mode 0, so the OS frees it if the process
/// dies. shortcut: no lock off Windows (std `File::try_lock` needs Rust 1.89, workspace is 1.88); this app ships on
/// Windows only. In-memory stores have no file and skip it.
#[allow(dead_code)] // held only for its Drop
struct ScanLock(#[cfg(windows)] Option<std::fs::File>);

impl ScanLock {
    #[cfg(windows)]
    fn take(store: &Store) -> Result<ScanLock, ScanError> {
        use std::os::windows::fs::OpenOptionsExt;
        let Some(db) = store.conn.path().filter(|p| !p.is_empty()) else {
            return Ok(ScanLock(None));
        };
        const ERROR_SHARING_VIOLATION: i32 = 32;
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(std::path::Path::new(db).with_file_name("scan.lock"))
        {
            Ok(f) => Ok(ScanLock(Some(f))),
            Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => Err(ScanError::Busy),
            Err(e) => Err(e.into()),
        }
    }

    #[cfg(not(windows))]
    fn take(_: &Store) -> Result<ScanLock, ScanError> {
        Ok(ScanLock())
    }
}

/// A corrupt database is a cache: zero the version so the next `Store::open` rebuilds it.
fn reset_if_corrupt(store: &Store, e: &ScanError) {
    if let ScanError::Db(rusqlite::Error::SqliteFailure(f, _)) = e {
        if f.code == rusqlite::ErrorCode::DatabaseCorrupt {
            let _ = store.conn.execute_batch("PRAGMA user_version = 0");
        }
    }
}

#[cfg(test)]
mod corrupt_tests {
    use super::*;

    #[test]
    fn a_corrupt_database_error_zeroes_the_schema_version() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("s.db")).unwrap();
        let ver = |s: &Store| {
            s.conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let err = |code| {
            ScanError::Db(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None,
            ))
        };
        reset_if_corrupt(&store, &err(rusqlite::ffi::SQLITE_CONSTRAINT));
        assert_ne!(ver(&store), 0, "other errors keep the cache");
        reset_if_corrupt(&store, &err(rusqlite::ffi::SQLITE_CORRUPT));
        assert_eq!(ver(&store), 0);
    }
}

/// The scan itself; `run_with` adds `Store::end_scan` on every outcome.
fn scan_body(
    src: &Source,
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
    procs: &dyn Fn() -> Vec<String>,
    skip_derived: bool,
) -> Result<Report, ScanError> {
    // 1. Process gate.
    if !skip_derived && pgp::running(&procs()) {
        return Err(ScanError::PgpRunning);
    }
    let mut report = Report::default();
    let mut t = Instant::now();

    // 2. Origins.
    let (mut loose, bsas, seps) = match src {
        Source::Mo2(inst) => {
            let (l, missing) = loose_origins(inst)?;
            report.missing_mods = missing;
            let (b, notes) = active_bsas(inst, &l);
            report.notes = notes;
            let s = separators(inst, &l)?;
            (l, b, s)
        }
        Source::Folder(p) => (
            vec![LooseOrigin {
                kind: OriginKind::Mod,
                name: p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                ),
                root: p.clone(),
                priority: 0,
            }],
            vec![],
            vec![],
        ),
    };
    // Plugins are not NIF-derived output and the load order must not shrink with the PGPatcher mods (ruling F-6).
    let plugin_origins = loose.clone();
    if skip_derived {
        // BSA ranks and separator priorities above were computed with these mods present, as MO2 sees them.
        loose.retain(|o| o.kind != OriginKind::Mod || pgp::signature(&o.root) == Signature::None);
    }
    store.begin_scan()?;
    let mut loose_ids = Vec::with_capacity(loose.len());
    // Whole-origin derived flag (Mod with a signature) and the parsed diff, per loose origin.
    let mut whole_derived = vec![false; loose.len()];
    let mut sig_any = vec![false; loose.len()];
    let mut diffs: Vec<Option<Diff>> = Vec::with_capacity(loose.len());
    for (i, o) in loose.iter().enumerate() {
        let sig = pgp::signature(&o.root);
        let state = match (o.kind, sig) {
            (_, Signature::None) => None,
            (OriginKind::Mod, Signature::Incomplete) => Some(DerivedState::Incomplete),
            (OriginKind::Mod, _) => Some(DerivedState::Ok),
            _ => Some(DerivedState::InOverwrite),
        };
        whole_derived[i] = !skip_derived && o.kind == OriginKind::Mod && sig != Signature::None;
        sig_any[i] = sig != Signature::None;
        // Without the PGPatcher output, the origin is recorded as ordinary; its derived paths are dropped below.
        let state = state.filter(|_| !skip_derived);
        let id = store.upsert_origin_staged(&OriginSpec {
            kind: kind_code(o.kind),
            name: o.name.clone(),
            root: o.root.display().to_string(),
            parent: None,
            priority: o.priority,
            derived_by: whole_derived[i].then_some(1),
            derived_state: state,
        })?;
        loose_ids.push(id);
        diffs.push(if sig_any[i] {
            pgp::read_diff(&o.root)
        } else {
            None
        });
        if let Some(state) = state {
            report.pgp.push(PgpOrigin {
                origin_id: id,
                name: o.name.clone(),
                signature: sig,
                state,
            });
        }
    }
    // Origins stay disabled while the run is in progress; `enable_run` is the commit point (before `delete_unseen`).
    let mut bsa_ids = Vec::with_capacity(bsas.len());
    for b in &bsas {
        let parent = b.providing_mod.as_ref().and_then(|m| {
            loose
                .iter()
                .position(|o| o.kind == OriginKind::Mod && &o.name == m)
                .map(|i| loose_ids[i])
        });
        let name = b
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        bsa_ids.push(store.upsert_origin_staged(&OriginSpec {
            kind: KIND_BSA,
            name,
            root: b.path.display().to_string(),
            parent,
            priority: b.rank,
            derived_by: None,
            derived_state: None,
        })?);
    }
    let mut sep_ids = vec![];
    if let Source::Mo2(inst) = src {
        for (name, prio) in &seps {
            sep_ids.push(store.upsert_origin_staged(&OriginSpec {
                kind: KIND_SEPARATOR,
                name: name.clone(),
                root: inst.mods.join(name).display().to_string(),
                parent: None,
                priority: *prio,
                derived_by: None,
                derived_state: None,
            })?);
        }
    }

    report.phase_ms.push(("origins", lap(&mut t)));

    // 3. Walk.
    let (mut copies, mut locs, opened, tex) = walk(&loose, &bsas, &mut report, progress);
    report.phase_ms.push(("walk", lap(&mut t)));
    let (mut parse_ms, mut write_ms) = (0u64, 0u64);
    let mut derived: Vec<bool> = copies
        .iter()
        .map(|c| match c.origin {
            OriginRef::Loose(i) => {
                whole_derived[i] || (sig_any[i] && pgp::is_derived_path(&c.path, diffs[i].as_ref()))
            }
            OriginRef::Bsa(_) => false,
        })
        .collect();
    if skip_derived {
        let mut keep = derived.iter().map(|d| !d);
        copies.retain(|_| keep.next().unwrap_or(true));
        let mut keep = derived.iter().map(|d| !d);
        locs.retain(|_| keep.next().unwrap_or(true));
        derived = vec![false; copies.len()];
    }
    let origin_id = |o: OriginRef| match o {
        OriginRef::Loose(i) => loose_ids[i],
        OriginRef::Bsa(i) => bsa_ids[i],
    };
    let origin_name = |o: OriginRef| match o {
        OriginRef::Loose(i) => loose[i].name.clone(),
        OriginRef::Bsa(i) => bsas[i].path.display().to_string(),
    };
    report.nifs = copies.len();

    // 4. Parse or reuse.
    let ti = Instant::now();
    let path_ids = store.path_ids(&copies.iter().map(|c| c.path.as_str()).collect::<Vec<_>>())?;
    let existing = store.existing_copies()?;
    let intern_ms = ti.elapsed().as_millis() as u64;
    // Source candidates of derived copies: every non-derived copy of a derived copy's (dup-stripped) path.
    let source_paths: HashSet<String> = copies
        .iter()
        .zip(&derived)
        .filter(|(_, d)| **d)
        .map(|(c, _)| pgp::dup_source(&c.path).unwrap_or_else(|| c.path.clone()))
        .collect();
    let candidate: Vec<bool> = copies
        .iter()
        .zip(&derived)
        .map(|(c, d)| !d && source_paths.contains(&c.path))
        .collect();
    let mut crc_known: HashMap<usize, u32> = HashMap::new();
    let mut work = vec![];
    for (i, c) in copies.iter().enumerate() {
        match existing.get(&(path_ids[i], origin_id(c.origin))) {
            Some(m) if m.size == Some(c.size) && m.mtime == Some(c.mtime) => {
                report.reused += 1;
                if let Some(crc) = m.crc32 {
                    crc_known.insert(i, crc);
                }
                if m.le_mesh {
                    report.le_meshes += 1;
                }
                match m.parse_status {
                    PARSE_NOT_SKYRIM => report.not_skyrim += 1,
                    PARSE_FAILED => report.problems.push((
                        format!("{} ({})", c.path, origin_name(c.origin)),
                        m.problem.clone().unwrap_or_default(),
                    )),
                    _ => {}
                }
            }
            _ => work.push(i),
        }
    }
    let total = work.len();
    let (read_loose_ns, read_bsa_ns, nif_parse_ns, write_ns) = (
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    );
    // Spec §2 step 3: a writer thread stores chunk N while chunk N+1 is parsed; the Store stays on that one thread.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<ChunkItem>>(2);
    let writer_store = &mut *store;
    let wns = &write_ns;
    let looped: Result<(), ScanError> = std::thread::scope(|s| {
        let writer = s.spawn(move || -> Result<(), ScanError> {
            for items in rx {
                let tw = Instant::now();
                writer_store.write_chunk(&items)?;
                wns.fetch_add(ns(tw), Ordering::Relaxed);
            }
            Ok(())
        });
        let mut out = Ok(());
        for (n, chunk) in work.chunks(CHUNK).enumerate() {
            if cancel.load(Ordering::Relaxed) {
                out = Err(ScanError::Cancelled);
                break;
            }
            progress(Progress {
                phase: "parse",
                done: n * CHUNK,
                total,
            });
            let results: Vec<(usize, CopyResult, Option<u32>)> = chunk
                .par_iter()
                .map(|&i| {
                    let t0 = Instant::now();
                    let (bytes, read_ns) = match &locs[i] {
                        Loc::Loose(p) => {
                            (std::fs::read(p).map_err(|e| e.to_string()), &read_loose_ns)
                        }
                        Loc::Bsa(b, e) => {
                            let bsa = &opened[*b];
                            (
                                bsa.read(&bsa.entries[*e]).map_err(|e| e.to_string()),
                                &read_bsa_ns,
                            )
                        }
                    };
                    read_ns.fetch_add(ns(t0), Ordering::Relaxed);
                    match bytes {
                        Ok(b) => {
                            let t1 = Instant::now();
                            let crc = candidate[i].then(|| crc32fast::hash(&b));
                            let r = scan_guarded(&b);
                            nif_parse_ns.fetch_add(ns(t1), Ordering::Relaxed);
                            (i, r, crc)
                        }
                        Err(e) => (i, CopyResult::ReadFailed(e), None),
                    }
                })
                .collect();
            let mut items = Vec::with_capacity(results.len());
            for (i, res, crc) in results {
                let c = &copies[i];
                report.parsed += 1;
                match &res {
                    CopyResult::Scanned(s) if s.le_mesh => report.le_meshes += 1,
                    CopyResult::NotSkyrim { .. } => report.not_skyrim += 1,
                    CopyResult::Failed(m) | CopyResult::ReadFailed(m) => report
                        .problems
                        .push((format!("{} ({})", c.path, origin_name(c.origin)), m.clone())),
                    _ => {}
                }
                if let Some(crc) = crc {
                    crc_known.insert(i, crc);
                }
                items.push(ChunkItem {
                    path_id: path_ids[i],
                    origin_id: origin_id(c.origin),
                    size: c.size,
                    mtime: c.mtime,
                    crc32: crc,
                    derived: derived[i],
                    result: res,
                });
            }
            // A closed channel means the writer stopped on an error; `join` returns it.
            if tx.send(items).is_err() {
                break;
            }
        }
        drop(tx);
        parse_ms = lap(&mut t);
        let w = writer
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p));
        w.and(out)
    });
    if let Err(e) = looped {
        if matches!(e, ScanError::Cancelled) {
            store.discard_pending()?;
        }
        return Err(e);
    }
    progress(Progress {
        phase: "parse",
        done: total,
        total,
    });

    // 5. Re-check the process gate: a run that overlapped PGPatcher keeps the previous derived rows.
    if !skip_derived && pgp::running(&procs()) {
        report.tainted = true;
        store.discard_pending()?;
        // A tainted run still returns a (flagged) result: its origins are listed, as before.
        enable_run(store, &loose_ids, &bsa_ids, &sep_ids)?;
        for p in &mut report.pgp {
            p.state = DerivedState::RunningAtScan;
            store.set_derived_state(p.origin_id, DerivedState::RunningAtScan)?;
        }
        return Ok(report);
    }
    let tc = Instant::now();
    store.commit_pending()?;
    let commit_ms = tc.elapsed().as_millis() as u64;
    write_ms += lap(&mut t);
    report.phase_ms.push(("parse", parse_ms));
    report.phase_ms.push(("write", write_ms));
    report.sub_ms.extend([
        ("intern", intern_ms),
        ("read_loose_cpu", read_loose_ns.into_inner() / 1_000_000),
        ("read_bsa_cpu", read_bsa_ns.into_inner() / 1_000_000),
        ("nif_parse_cpu", nif_parse_ns.into_inner() / 1_000_000),
        ("write_chunk", write_ns.into_inner() / 1_000_000),
        ("commit_pending", commit_ms),
    ]);

    // 6. Winners and the derived layer.
    progress(Progress {
        phase: "resolve",
        done: 0,
        total: 1,
    });
    let tr = Instant::now();
    let ids_by_key = store.copy_ids()?;
    let ids: Vec<i64> = copies
        .iter()
        .enumerate()
        .map(|(i, c)| {
            ids_by_key
                .get(&(path_ids[i], origin_id(c.origin)))
                .copied()
                .ok_or_else(|| ScanError::Io(std::io::Error::other("copy row missing after scan")))
        })
        .collect::<Result<_, _>>()?;
    let game = winners(&copies, &loose, &bsas, &|_| false);
    let nd: Vec<usize> = (0..copies.len()).filter(|&i| !derived[i]).collect();
    let nd_copies: Vec<Copy> = nd.iter().map(|&i| copies[i].clone()).collect();
    let source: HashMap<String, usize> = winners(&nd_copies, &loose, &bsas, &|_| false)
        .into_iter()
        .map(|(p, (j, _))| (p, nd[j]))
        .collect();
    let pid_of: HashMap<&str, i64> = copies
        .iter()
        .zip(&path_ids)
        .map(|(c, &p)| (c.path.as_str(), p))
        .collect();
    let game_rows: HashMap<i64, (i64, u32)> = game
        .iter()
        .map(|(p, &(i, n))| (pid_of[p.as_str()], (ids[i], n)))
        .collect();
    let src_rows: HashMap<i64, i64> = source
        .iter()
        .map(|(p, &i)| (pid_of[p.as_str()], ids[i]))
        .collect();
    store.set_winners(&game_rows, &src_rows)?;
    report
        .sub_ms
        .push(("winners", tr.elapsed().as_millis() as u64));
    let mut resolve_ms = lap(&mut t);

    // Texture winners (game view only; stat-only data from the walk).
    let tex_pids = store.path_ids(&tex.iter().map(|c| c.path.as_str()).collect::<Vec<_>>())?;
    let tex_win = winners(&tex, &loose, &bsas, &|_| false);
    let tex_pid_of: HashMap<&str, i64> = tex
        .iter()
        .zip(&tex_pids)
        .map(|(c, &p)| (c.path.as_str(), p))
        .collect();
    let tex_rows: Vec<(i64, i64, u64, i64)> = tex
        .iter()
        .zip(&tex_pids)
        .map(|(c, &p)| (p, origin_id(c.origin), c.size, c.mtime))
        .collect();
    let tex_win_rows: HashMap<i64, (i64, u32)> = tex_win
        .iter()
        .map(|(p, &(i, n))| (tex_pid_of[p.as_str()], (origin_id(tex[i].origin), n)))
        .collect();
    store.set_textures(&tex_rows, &tex_win_rows)?;
    let textures_ms = lap(&mut t);

    // pgp_diff: higher-priority origins overwrite lower ones on the same key.
    let mut order: Vec<usize> = (0..loose.len()).filter(|&i| diffs[i].is_some()).collect();
    order.sort_by_key(|&i| loose[i].priority);
    let mut diff_rows: HashMap<String, (u32, u32)> = HashMap::new();
    for i in order {
        let d = diffs[i].as_ref().unwrap();
        for (k, &o) in &d.crc_orig {
            diff_rows.insert(k.clone(), (o, d.crc_patched.get(k).copied().unwrap_or(0)));
        }
    }
    store.set_pgp_diff(&diff_rows)?;

    // Non-derived copies by path, for the "deeper copies" CRC comparison.
    let mut by_path: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in &nd {
        by_path.entry(copies[i].path.as_str()).or_default().push(i);
    }
    let mut new_crcs: HashMap<i64, u32> = HashMap::new();
    let mut crc_of = |i: usize| -> Option<u32> {
        if let Some(c) = crc_known.get(&i) {
            return Some(*c);
        }
        let Loc::Loose(p) = &locs[i] else {
            return None; // BSA source: unverified (05 §5.1)
        };
        let c = crc32fast::hash(&std::fs::read(p).ok()?);
        crc_known.insert(i, c);
        new_crcs.insert(ids[i], c);
        Some(c)
    };
    let mut rows = Vec::with_capacity(copies.len());
    for (i, c) in copies.iter().enumerate() {
        if !derived[i] {
            rows.push(DeriveRow {
                copy_id: ids[i],
                is_derived: false,
                source_copy_id: None,
                derive_check: None,
            });
            continue;
        }
        let dup = pgp::dup_source(&c.path);
        let sp = dup.clone().unwrap_or_else(|| c.path.clone());
        let src_i = source.get(&sp).copied();
        let check = if dup.is_some() {
            DeriveCheck::DupOf
        } else {
            let diff_crc = match c.origin {
                OriginRef::Loose(o) => diffs[o]
                    .as_ref()
                    .and_then(|d| d.crc_orig.get(&c.path).copied()),
                OriginRef::Bsa(_) => None,
            };
            match (diff_crc, src_i) {
                (None, _) | (_, None) => DeriveCheck::UnknownSource,
                (Some(d), Some(s)) => {
                    let sc = crc_of(s);
                    let deeper: Vec<u32> = if sc.is_some_and(|x| x != d) {
                        by_path[sp.as_str()]
                            .iter()
                            .filter(|&&j| j != s)
                            .filter_map(|&j| crc_of(j))
                            .collect()
                    } else {
                        vec![]
                    };
                    pgp::check(Some(d), sc, &deeper)
                }
            }
        };
        rows.push(DeriveRow {
            copy_id: ids[i],
            is_derived: true,
            source_copy_id: src_i.map(|s| ids[s]),
            derive_check: Some(check_code(check)),
        });
    }
    store.set_derived(&rows, &new_crcs)?;

    // 7. Drop copies not seen in this walk.
    let tdl = Instant::now();
    let seen: HashSet<i64> = ids.iter().copied().collect();
    // Commit point: only a run that got here makes its origins readable (the plugin pass below needs them enabled).
    enable_run(store, &loose_ids, &bsa_ids, &sep_ids)?;
    store.delete_unseen(&seen)?;
    report
        .sub_ms
        .push(("delete_unseen", tdl.elapsed().as_millis() as u64));
    resolve_ms += lap(&mut t);
    report.phase_ms.push(("resolve", resolve_ms));
    report.phase_ms.push(("textures", textures_ms));
    progress(Progress {
        phase: "resolve",
        done: 1,
        total: 1,
    });

    // 8. Plugins (esp spec §2.2), after the NIF/texture pass, same cancel and progress. A Folder has no load order.
    match src {
        Source::Mo2(inst) => {
            match crate::plugins::scan_plugins(inst, &plugin_origins, store, cancel, progress) {
                Ok(p) => {
                    report.phase_ms.push(("plugins", p.ms));
                    report.sub_ms.extend(p.sub_ms.iter().copied());
                    report.plugins = Some(p);
                }
                Err(ScanError::Cancelled) => report.notes.push(PLUGINS_CANCELLED.into()),
                Err(e) => return Err(e),
            }
        }
        Source::Folder(_) => store.unload_plugins()?,
    }
    Ok(report)
}

fn enable_run(store: &mut Store, loose: &[i64], bsa: &[i64], sep: &[i64]) -> Result<(), ScanError> {
    let all: Vec<i64> = loose.iter().chain(bsa).chain(sep).copied().collect();
    store.enable_origins(&all)
}

/// Step 3: loose NIFs per origin and NIF entries per BSA (FaceGen excluded), one copy per (path, origin).
fn walk(
    loose: &[LooseOrigin],
    bsas: &[ActiveBsa],
    report: &mut Report,
    progress: &dyn Fn(Progress),
) -> (Vec<Copy>, Vec<Loc>, Vec<Bsa>, Vec<Copy>) {
    let total = loose.len() + bsas.len();
    let mut copies = vec![];
    let mut locs = vec![];
    let mut tex: Vec<Copy> = vec![];
    let mut seen: HashSet<(String, OriginRef)> = HashSet::new();
    for (i, o) in loose.iter().enumerate() {
        progress(Progress {
            phase: "walk",
            done: i,
            total,
        });
        for f in vfs::loose::walk(&o.root, &["meshes"], &["nif"]) {
            let origin = OriginRef::Loose(i);
            if !seen.insert((f.norm.clone(), origin)) {
                continue;
            }
            copies.push(Copy {
                path: f.norm,
                origin,
                size: f.size,
                mtime: f.mtime,
            });
            locs.push(Loc::Loose(f.abs));
        }
        // Textures: directory-entry stat only, never the bytes. Distinct paths per origin by construction.
        for f in vfs::loose::walk(&o.root, &["textures"], &["dds"]) {
            tex.push(Copy {
                path: f.norm,
                origin: OriginRef::Loose(i),
                size: f.size,
                mtime: f.mtime,
            });
        }
    }
    let mut opened = vec![];
    for (i, b) in bsas.iter().enumerate() {
        progress(Progress {
            phase: "walk",
            done: loose.len() + i,
            total,
        });
        let bsa = match Bsa::open(&b.path) {
            Ok(x) => x,
            Err(e) => {
                report
                    .problems
                    .push((b.path.display().to_string(), e.to_string()));
                continue;
            }
        };
        let mtime = file_mtime(&b.path);
        let bi = opened.len();
        for e in &bsa.entries {
            if e.norm.starts_with("textures/") && e.norm.ends_with(".dds") && !is_facegen(&e.norm) {
                tex.push(Copy {
                    path: e.norm.clone(),
                    origin: OriginRef::Bsa(i),
                    size: e.size as u64,
                    mtime,
                });
            }
        }
        for (ei, e) in bsa.entries.iter().enumerate() {
            if !e.norm.starts_with("meshes/") || !e.norm.ends_with(".nif") || is_facegen(&e.norm) {
                continue;
            }
            let origin = OriginRef::Bsa(i);
            if !seen.insert((e.norm.clone(), origin)) {
                continue;
            }
            copies.push(Copy {
                path: e.norm.clone(),
                origin,
                size: e.size as u64,
                mtime,
            });
            locs.push(Loc::Bsa(bi, ei));
        }
        opened.push(bsa);
    }
    progress(Progress {
        phase: "walk",
        done: total,
        total,
    });
    (copies, locs, opened, tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_panic_becomes_a_failed_copy() {
        let r = guarded(|| panic!("boom"));
        assert!(
            matches!(&r, CopyResult::Failed(m) if m.contains("parser panic") && m.contains("boom")),
            "{r:?}"
        );
        let r = guarded(|| panic!("{}", String::from("owned")));
        assert!(
            matches!(&r, CopyResult::Failed(m) if m.contains("owned")),
            "{r:?}"
        );
    }
}
