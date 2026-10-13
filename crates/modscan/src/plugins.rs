//! The plugin pass (esp scanner spec §2.2): active plugins found through the MO2 origins, load-order FormIDs.
use crate::archives::active_plugins;
use crate::mo2::{Instance, LooseOrigin};
use crate::plugin_store::{PluginFile, PluginRow};
use crate::scan::{file_mtime, Progress};
use crate::store::Store;
use crate::ScanError;
use esp::PluginFacts;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

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

/// Plugins parsed per progress step and cancel check.
const CHUNK: usize = 64;
/// Marker error of a read skipped because the scan was cancelled; never stored.
const CANCELLED: &str = "cancelled";

/// The plugin pass's numbers for the scan details line and the timing entry (ruling R6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginStats {
    /// Active plugins found on disk.
    pub n: usize,
    /// Of those, kept from the store (same path, size, mtime).
    pub cached: usize,
    /// Record copies stored for the active plugins.
    pub records: usize,
    pub ms: u64,
    /// (plugin, message): not found, parse errors, missing masters, load-order limits. Every scan, cached or not.
    pub problems: Vec<(String, String)>,
}

/// An active plugin found on disk.
struct Found {
    name: String,
    path: PathBuf,
    size: u64,
    mtime: i64,
}

fn read_plugin(a: &Found, cancel: &AtomicBool) -> Result<PluginFacts, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    let bytes = std::fs::read(&a.path).map_err(|e| format!("read failed: {e}"))?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| esp::parse(&a.name, &bytes)))
        .map_err(|_| "plugin parser panic".to_string())
}

/// The plugin pass (spec §2.2): find the active plugins through the MO2 origins, parse the changed ones in parallel
/// (one plugin per rayon task), drop the ones that left, then resolve load-order FormIDs and winners when anything
/// changed (ruling R1).
pub fn scan_plugins(
    inst: &Instance,
    loose: &[LooseOrigin],
    store: &mut Store,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
) -> Result<PluginStats, ScanError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ScanError::Cancelled);
    }
    let t = Instant::now();
    let mut st = PluginStats::default();
    let files = plugin_files(loose);
    let mut active: Vec<Found> = vec![];
    for p in active_plugins(inst) {
        let Some((path, name)) = files.get(&p.name) else {
            if !p.optional {
                st.problems.push((
                    p.name.clone(),
                    "not found in the game Data folder or any enabled mod".into(),
                ));
            }
            continue;
        };
        match std::fs::metadata(path) {
            Ok(md) => active.push(Found {
                name: name.clone(),
                path: path.clone(),
                size: md.len(),
                mtime: file_mtime(path),
            }),
            Err(e) => st.problems.push((name.clone(), e.to_string())),
        }
    }
    st.n = active.len();

    let before = store.plugin_rows()?;
    let work: Vec<usize> = (0..active.len())
        .filter(|&i| {
            let a = &active[i];
            before.get(&a.name.to_ascii_lowercase()).is_none_or(|r| {
                r.path != a.path.display().to_string()
                    || r.size != Some(a.size)
                    || r.mtime != a.mtime
            })
        })
        .collect();
    st.cached = active.len() - work.len();
    for (k, chunk) in work.chunks(CHUNK).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }
        progress(Progress {
            phase: "plugins",
            done: k * CHUNK,
            total: work.len(),
        });
        let read: Vec<(usize, Result<PluginFacts, String>)> = chunk
            .par_iter()
            .map(|&i| (i, read_plugin(&active[i], cancel)))
            .collect();
        if cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }
        for (i, r) in read {
            let a = &active[i];
            let (facts, size) = match r {
                Ok(f) => (f, Some(a.size)),
                Err(e) => (
                    PluginFacts {
                        name: a.name.clone(),
                        errors: vec![e],
                        ..Default::default()
                    },
                    None,
                ),
            };
            let path = a.path.display().to_string();
            store.write_plugin(
                &PluginFile {
                    name: &a.name,
                    path: &path,
                    size,
                    mtime: a.mtime,
                },
                &facts,
            )?;
        }
    }
    progress(Progress {
        phase: "plugins",
        done: work.len(),
        total: work.len(),
    });

    if cancel.load(Ordering::Relaxed) {
        return Err(ScanError::Cancelled);
    }
    let names: Vec<String> = active.iter().map(|a| a.name.clone()).collect();
    let removed = store.retain_plugins(&names)?;
    let rows = store.plugin_rows()?;
    let order: Vec<&PluginRow> = names
        .iter()
        .filter_map(|n| rows.get(&n.to_ascii_lowercase()))
        .collect();
    let pre = prefixes(&order.iter().map(|r| r.light).collect::<Vec<_>>());
    let index: HashMap<String, usize> = order
        .iter()
        .enumerate()
        .map(|(i, r)| (r.name.to_ascii_lowercase(), i))
        .collect();
    let mut map: Vec<[i64; 4]> = vec![];
    for (i, r) in order.iter().enumerate() {
        for e in r.error_text.iter().flat_map(|t| t.lines()) {
            st.problems.push((r.name.clone(), e.to_string()));
        }
        let Some((base, mask)) = pre[i] else {
            st.problems.push((
                r.name.clone(),
                "past the load-order limit (254 full / 4096 light plugins): its records are not mapped".into(),
            ));
            continue;
        };
        for (idx, m) in r.masters.iter().enumerate() {
            match index.get(&m.to_ascii_lowercase()).and_then(|&j| pre[j]) {
                Some((b, k)) => map.push([r.id, idx as i64, i64::from(b), i64::from(k)]),
                None => st
                    .problems
                    .push((r.name.clone(), format!("missing master {m}"))),
            }
        }
        map.push([
            r.id,
            r.masters.len() as i64,
            i64::from(base),
            i64::from(mask),
        ]);
    }
    let same = work.is_empty()
        && removed == 0
        && order
            .iter()
            .enumerate()
            .all(|(i, r)| r.lo_index == Some(i as i64));
    if !same {
        let lo: Vec<(i64, i64)> = order
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, i as i64))
            .collect();
        store.resolve_plugins(&lo, &map)?;
    }
    st.records = store.plugin_record_count()?;
    st.ms = t.elapsed().as_millis() as u64;
    Ok(st)
}
