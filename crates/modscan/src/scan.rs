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
use std::sync::atomic::{AtomicBool, Ordering};
use vfs::bsa::Bsa;

const CHUNK: usize = 512;

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
}

enum Loc {
    Loose(PathBuf),
    Bsa(usize, usize),
}

fn file_mtime(p: &std::path::Path) -> i64 {
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
    // 1. Process gate.
    if pgp::running(&procs()) {
        return Err(ScanError::PgpRunning);
    }
    let mut report = Report::default();

    // 2. Origins.
    let (loose, bsas, seps) = match src {
        Source::Mo2(inst) => {
            let (l, missing) = loose_origins(inst)?;
            report.missing_mods = missing;
            let b = active_bsas(inst, &l);
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
        whole_derived[i] = o.kind == OriginKind::Mod && sig != Signature::None;
        sig_any[i] = sig != Signature::None;
        let id = store.upsert_origin(&OriginSpec {
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
        bsa_ids.push(store.upsert_origin(&OriginSpec {
            kind: KIND_BSA,
            name,
            root: b.path.display().to_string(),
            parent,
            priority: b.rank,
            derived_by: None,
            derived_state: None,
        })?);
    }
    if let Source::Mo2(inst) = src {
        for (name, prio) in &seps {
            store.upsert_origin(&OriginSpec {
                kind: KIND_SEPARATOR,
                name: name.clone(),
                root: inst.mods.join(name).display().to_string(),
                parent: None,
                priority: *prio,
                derived_by: None,
                derived_state: None,
            })?;
        }
    }

    // 3. Walk.
    let (copies, locs, opened) = walk(&loose, &bsas, &mut report, progress);
    let derived: Vec<bool> = copies
        .iter()
        .map(|c| match c.origin {
            OriginRef::Loose(i) => {
                whole_derived[i] || (sig_any[i] && pgp::is_derived_path(&c.path, diffs[i].as_ref()))
            }
            OriginRef::Bsa(_) => false,
        })
        .collect();
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
    let path_ids = store.path_ids(&copies.iter().map(|c| c.path.as_str()).collect::<Vec<_>>())?;
    let existing = store.existing_copies()?;
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
            Some(m) if m.size == c.size && m.mtime == c.mtime => {
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
    for (n, chunk) in work.chunks(CHUNK).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            store.discard_pending()?;
            return Err(ScanError::Cancelled);
        }
        progress(Progress {
            phase: "parse",
            done: n * CHUNK,
            total,
        });
        let results: Vec<(usize, CopyResult, Option<u32>)> = chunk
            .par_iter()
            .map(|&i| {
                let bytes = match &locs[i] {
                    Loc::Loose(p) => std::fs::read(p).map_err(|e| e.to_string()),
                    Loc::Bsa(b, e) => {
                        let bsa = &opened[*b];
                        bsa.read(&bsa.entries[*e]).map_err(|e| e.to_string())
                    }
                };
                match bytes {
                    Ok(b) => {
                        let crc = candidate[i].then(|| crc32fast::hash(&b));
                        (i, CopyResult::from(nif::scan::scan(&b)), crc)
                    }
                    Err(e) => (i, CopyResult::Failed(e), None),
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
                CopyResult::Failed(m) => report
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
        store.write_chunk(&items)?;
    }
    progress(Progress {
        phase: "parse",
        done: total,
        total,
    });

    // 5. Re-check the process gate: a run that overlapped PGPatcher keeps the previous derived rows.
    if pgp::running(&procs()) {
        report.tainted = true;
        store.discard_pending()?;
        for p in &mut report.pgp {
            p.state = DerivedState::RunningAtScan;
            store.set_derived_state(p.origin_id, DerivedState::RunningAtScan)?;
        }
        return Ok(report);
    }
    store.commit_pending()?;

    // 6. Winners and the derived layer.
    progress(Progress {
        phase: "resolve",
        done: 0,
        total: 1,
    });
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
    let seen: HashSet<i64> = ids.iter().copied().collect();
    store.delete_unseen(&seen)?;
    progress(Progress {
        phase: "resolve",
        done: 1,
        total: 1,
    });
    Ok(report)
}

/// Step 3: loose NIFs per origin and NIF entries per BSA (FaceGen excluded), one copy per (path, origin).
fn walk(
    loose: &[LooseOrigin],
    bsas: &[ActiveBsa],
    report: &mut Report,
    progress: &dyn Fn(Progress),
) -> (Vec<Copy>, Vec<Loc>, Vec<Bsa>) {
    let total = loose.len() + bsas.len();
    let mut copies = vec![];
    let mut locs = vec![];
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
    (copies, locs, opened)
}
