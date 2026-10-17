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
pub fn walk(root: &Path, top: &[&str], ext: &[&str]) -> Vec<LooseFile> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut out = vec![];
    for d in rd.filter_map(Result::ok) {
        let name = d.file_name().to_string_lossy().to_ascii_lowercase();
        if !top.contains(&name.as_str()) || !d.path().is_dir() {
            continue;
        }
        // MO2/Vortex deploys use junctions and symlinks: follow them wherever they point; the
        // path stays the link path. walkdir's ancestor check reports cycles as Err (dropped below).
        let it = walkdir::WalkDir::new(d.path())
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                !(e.file_type().is_dir() && e.file_name().eq_ignore_ascii_case("facegendata"))
            });
        for e in it
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
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
                Err(_) => continue,
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
    out
}
