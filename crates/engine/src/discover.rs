//! Input discovery: turns the files and folders given on the command line into `InputFile`s,
//! each with its game-relative path and the DDS path it will be written to.
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const IMAGE_EXTS: [&str; 4] = ["png", "tga", "tif", "tiff"];

/// Folder the engine writes provenance to; never treated as input.
const SDC_DIR: &str = "_sdc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Image,
    Dds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputFile {
    /// The file as given (or found under a given folder).
    pub path: PathBuf,
    /// Path relative to the folder it was found in (just the file name for an explicit file).
    pub rel: PathBuf,
    /// Lower-case, '/'-separated path from the last `textures` component, else `rel`.
    pub game_path: String,
    /// `<output_root>/<rel>` with a `.dds` extension.
    pub output: PathBuf,
    pub kind: InputKind,
}

fn ext_lower(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn game_path(abs: &Path, rel: &Path) -> String {
    let comps: Vec<String> = abs
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    match comps.iter().rposition(|c| c == "textures") {
        Some(i) => comps[i..].join("/"),
        None => rel
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase(),
    }
}

/// Component-wise prefix test that ignores case, as NTFS does.
fn is_under(path: &Path, root: &Path) -> bool {
    let lower = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    lower(path).starts_with(&lower(root))
}

fn make(path: PathBuf, rel: PathBuf, output_root: &Path) -> InputFile {
    let kind = if ext_lower(&path) == "dds" {
        InputKind::Dds
    } else {
        InputKind::Image
    };
    let abs = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
    let game_path = game_path(&abs, &rel);
    let output = output_root.join(&rel).with_extension("dds");
    InputFile {
        path,
        rel,
        game_path,
        output,
        kind,
    }
}

/// Finds inputs. Folders are walked one level deep unless `recursive`; `_sdc` folders and
/// anything under `output_root` are skipped, and unknown extensions inside folders are ignored.
/// Explicit file inputs are always included. The result is sorted by `rel`.
pub fn discover(
    inputs: &[PathBuf],
    output_root: &Path,
    recursive: bool,
) -> std::io::Result<Vec<InputFile>> {
    let out_abs = std::path::absolute(output_root)?;
    let mut files = vec![];
    for input in inputs {
        if input.is_dir() {
            let max_depth = if recursive { usize::MAX } else { 1 };
            let walker = walkdir::WalkDir::new(input)
                .min_depth(1)
                .max_depth(max_depth);
            let keep = |e: &walkdir::DirEntry| {
                if !e.file_type().is_dir() {
                    return true;
                }
                e.file_name() != SDC_DIR
                    && !std::path::absolute(e.path()).is_ok_and(|a| is_under(&a, &out_abs))
            };
            for e in walker.into_iter().filter_entry(keep) {
                let e = e.map_err(std::io::Error::other)?;
                let p = e.path();
                if !e.file_type().is_file() || is_under(&std::path::absolute(p)?, &out_abs) {
                    continue;
                }
                let ext = ext_lower(p);
                if ext == "dds" || IMAGE_EXTS.contains(&ext.as_str()) {
                    let rel = p
                        .strip_prefix(input)
                        .map_err(std::io::Error::other)?
                        .to_path_buf();
                    files.push(make(p.to_path_buf(), rel, output_root));
                }
            }
        } else {
            let name = input
                .file_name()
                .ok_or_else(|| std::io::Error::other("input has no file name"))?;
            files.push(make(input.clone(), PathBuf::from(name), output_root));
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// Spec §3.7, §4.4: an input whose stem ends in a preset suffix alias or an exporter vocabulary term is
/// written under its Skyrim name: `iron_rmaosultrahigh.png` → `iron_rmaos.dds`, `iron_normalgl.png` →
/// `iron_n.dds`, `iron_basecolor.png` → `iron.dds`. Raw packing inputs keep their name (they are never
/// written on their own). Only `output` changes; `rel` and `game_path` keep the source name, so rule globs
/// match what is on disk. Run before grouping.
pub fn canonicalise_outputs(files: &mut [InputFile], aliases: &BTreeMap<String, String>) {
    use crate::group::{split_suffix_with, vocab_of, VocabRole};
    for f in files {
        let stem = f
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let split = split_suffix_with(&stem, aliases);
        let suffixed = || {
            split
                .suffix
                .as_ref()
                .map(|c| format!("{}_{c}.dds", split.base))
        };
        let name = match (&split.alias, vocab_of(&stem)) {
            (Some(_), _) => suffixed(),
            (None, Some(v)) => match v.role {
                VocabRole::PackingInput(_) => None,
                VocabRole::Colour => Some(format!("{}.dds", split.base)),
                VocabRole::Normal(_) | VocabRole::Emissive => suffixed(),
            },
            (None, None) => None,
        };
        if let Some(n) = name {
            f.output.set_file_name(n);
        }
    }
}
