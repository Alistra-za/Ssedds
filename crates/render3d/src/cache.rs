//! Housekeeping of the on-disk shader cache (`<key>.cso` + `<key>.json` per stage, written by `compile`): a hit
//! refreshes the entry's age, and one background pass at renderer start removes what is stale or over the size
//! cap (03 §5). Deleting while another process reads is safe: a missing or torn pair is a miss and recompiles.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Refreshes the modified time of a cache entry's `.json` (the entry's age; access time is not used because
/// NTFS often has last-access updates off). Best effort.
pub fn touch(json: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(json) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneLimits {
    pub max_bytes: u64,
    pub max_age_days: u32,
    pub stale_tmp_days: u32,
}

impl Default for PruneLimits {
    fn default() -> PruneLimits {
        PruneLimits {
            max_bytes: 512 * 1024 * 1024,
            max_age_days: 90,
            stale_tmp_days: 1,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Files deleted.
    pub removed: usize,
    /// Bytes of the cache files left (entries, young temp files and young orphans).
    pub bytes_after: u64,
}

/// A cache key stem: 64 lowercase hex chars (`hex::encode(sha256)`, see `ProgramKey`). Nothing else is ours.
fn is_key(stem: &str) -> bool {
    stem.len() == 64 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// One entry: `.json` first, then `.cso`, so a reader sees a plain miss. False when the `.json` stayed.
fn drop_entry(files: &[PathBuf], removed: &mut usize) -> bool {
    let Some((first, rest)) = files.split_first() else {
        return false;
    };
    if std::fs::remove_file(first).is_err() {
        return false;
    }
    *removed += 1;
    for f in rest {
        if std::fs::remove_file(f).is_ok() {
            *removed += 1;
        }
    }
    true
}

/// Removes, in this order: `.tmp-*` older than `stale_tmp_days`; a `.cso` with no `.json` older than
/// `stale_tmp_days`; entries whose `.json` is older than `max_age_days`; then the oldest entries until the files
/// left total at most `max_bytes`. Top level of `dir` only; only `.tmp-*` and
/// `<64 lowercase hex>.json|.cso` files are touched or counted, anything else (e.g. `settings.json`) is left alone.
pub fn prune(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return PruneReport::default();
    };
    let age = |t: SystemTime| now.duration_since(t).unwrap_or(Duration::ZERO);
    let days = |d: u32| Duration::from_secs(u64::from(d) * 86_400);
    type Part = (PathBuf, u64, SystemTime);
    let mut parts: HashMap<String, [Option<Part>; 2]> = HashMap::new(); // stem -> [json, cso]
    let (mut removed, mut total) = (0usize, 0u64);
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if !md.is_file() {
            continue;
        }
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let (len, modified) = (md.len(), md.modified().unwrap_or(now));
        if name.starts_with(".tmp-") {
            if age(modified) > days(limits.stale_tmp_days) && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            } else {
                total += len;
            }
        } else if let Some(stem) = name.strip_suffix(".json").filter(|s| is_key(s)) {
            parts.entry(stem.to_string()).or_default()[0] = Some((path, len, modified));
        } else if let Some(stem) = name.strip_suffix(".cso").filter(|s| is_key(s)) {
            parts.entry(stem.to_string()).or_default()[1] = Some((path, len, modified));
        }
    }
    // (json mtime, bytes, [json, cso] paths)
    let mut live: Vec<(SystemTime, u64, Vec<PathBuf>)> = vec![];
    for [json, cso] in parts.into_values() {
        match (json, cso) {
            (Some(j), c) => {
                let mut files = vec![j.0];
                let mut bytes = j.1;
                if let Some(c) = c {
                    files.push(c.0);
                    bytes += c.1;
                }
                if age(j.2) > days(limits.max_age_days) && drop_entry(&files, &mut removed) {
                    continue;
                }
                total += bytes;
                live.push((j.2, bytes, files));
            }
            (None, Some(c)) => {
                if age(c.2) > days(limits.stale_tmp_days) && std::fs::remove_file(&c.0).is_ok() {
                    removed += 1;
                } else {
                    total += c.1;
                }
            }
            (None, None) => {}
        }
    }
    live.sort_by_key(|e| e.0);
    for (_, bytes, files) in live {
        if total <= limits.max_bytes {
            break;
        }
        if drop_entry(&files, &mut removed) {
            total -= bytes;
        }
    }
    PruneReport {
        removed,
        bytes_after: total,
    }
}

/// One prune per process, on its own thread, with the default limits and the real clock. Called by
/// `Renderer::new`; never run on the UI thread.
pub fn prune_in_background(dir: PathBuf) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("shader-cache-prune".into())
            .spawn(move || {
                prune_logged(&dir, &PruneLimits::default(), SystemTime::now());
            });
    });
}

/// Size of the top-level files of `dir` (what `prune` counts).
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// [`prune`] plus one `prune` line in the timing log.
pub fn prune_logged(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport {
    let t0 = std::time::Instant::now();
    let before = dir_bytes(dir);
    let rep = prune(dir, limits, now);
    crate::perf_log::emit(
        "prune",
        serde_json::json!({
            "removed": rep.removed,
            "bytes_before": before,
            "bytes_after": rep.bytes_after,
            "ms": crate::perf_log::ms_since(t0),
        }),
    );
    rep
}
