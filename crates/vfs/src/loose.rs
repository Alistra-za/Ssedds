use nif::path::{canon_mesh, is_facegen};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct LooseFile {
    pub norm: String,
    pub abs: PathBuf,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub mtime: i64,
}

/// Walks `root/<top>/**` for files with one of `ext` (lowercase, no dot). Case-insensitive on folder names.
/// Returns the files and every I/O error as `(path, message)`; a root that does not exist is empty, not an error.
pub fn walk(root: &Path, top: &[&str], ext: &[&str]) -> (Vec<LooseFile>, Vec<(String, String)>) {
    let mut out = vec![];
    let mut errs = vec![];
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (out, errs),
        Err(e) => {
            errs.push((root.display().to_string(), e.to_string()));
            return (out, errs);
        }
    };
    for d in rd {
        let d = match d {
            Ok(d) => d,
            Err(e) => {
                errs.push((root.display().to_string(), e.to_string()));
                continue;
            }
        };
        let name = d.file_name().to_string_lossy().to_ascii_lowercase();
        if !top.contains(&name.as_str()) {
            continue;
        }
        // metadata follows links: a broken link or inaccessible entry is reported, a plain file is skipped
        match std::fs::metadata(d.path()) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => continue,
            Err(e) => {
                errs.push((d.path().display().to_string(), e.to_string()));
                continue;
            }
        }
        // MO2/Vortex deploys use junctions and symlinks: follow them wherever they point; the
        // path stays the link path. walkdir's ancestor check reports cycles as Err (reported below).
        let it = walkdir::WalkDir::new(d.path())
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                // prune only the actor facegen folder (is_facegen), not any folder with that name
                !(e.file_type().is_dir()
                    && e.file_name().eq_ignore_ascii_case("facegendata")
                    && e.path().strip_prefix(root).is_ok_and(|r| {
                        is_facegen(&format!("{}/", canon_mesh(&r.to_string_lossy())))
                    }))
            });
        for e in it {
            let e = match e {
                Ok(e) => e,
                Err(e) => {
                    let dp = d.path();
                    let p = e.path().unwrap_or(&dp);
                    errs.push((p.display().to_string(), e.to_string()));
                    continue;
                }
            };
            if !e.file_type().is_file() {
                continue;
            }
            let p = e.path();
            let ok = p
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|x| ext.contains(&x.as_str()));
            if !ok {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap().to_string_lossy();
            let norm = canon_mesh(&rel);
            if is_facegen(&norm) {
                continue;
            }
            let md = match e.metadata() {
                Ok(m) => m,
                Err(e) => {
                    errs.push((p.display().to_string(), e.to_string()));
                    continue;
                }
            };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            out.push(LooseFile {
                norm,
                abs: p.to_path_buf(),
                size: md.len(),
                mtime,
            });
        }
    }
    out.sort_by(|a, b| a.norm.cmp(&b.norm));
    (out, errs)
}
