//! Input discovery: turns the files and folders given on the command line into `InputFile`s,
//! each with its game-relative path and the DDS path it will be written to.
use crate::codes::{Code, Warning};
use crate::presets::ReadAs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const IMAGE_EXTS: [&str; 14] = [
    "png", "tga", "tif", "tiff", "exr", "hdr", "bmp", "jpg", "jpeg", "webp", "psd", "psb", "kra",
    "ora",
];

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
    /// F3b: the `rel` found on disk when an `[inputs]` alias renamed it (`rel` then holds the alias). Keys
    /// (`[files]`, set edits, output paths) stay on the real file through it (`overrides::project_rel`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_rel: Option<PathBuf>,
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
        real_rel: None,
    }
}

/// Finds inputs. Folders are walked one level deep unless `recursive`; `_sdc` folders and
/// anything under an `output_root` strictly inside the folder are skipped (an output root equal to the folder, in
/// place, or above it skips nothing: Note BC), and unknown extensions inside folders are ignored.
/// Explicit file inputs are always included. The result is sorted by `rel`.
pub fn discover(
    inputs: &[PathBuf],
    output_root: &Path,
    recursive: bool,
) -> std::io::Result<Vec<InputFile>> {
    discover_reporting(inputs, output_root, recursive).map(|(f, _)| f)
}

/// `discover`, plus each folder entry it could not read (walkdir's message, which names the path).
pub fn discover_reporting(
    inputs: &[PathBuf],
    output_root: &Path,
    recursive: bool,
) -> std::io::Result<(Vec<InputFile>, Vec<String>)> {
    let out_abs = std::path::absolute(output_root)?;
    let mut files = vec![];
    let mut skipped = vec![];
    // Trace-debug #13 (Note BQ): a file reached through two inputs is kept once, through the first.
    let mut seen = std::collections::HashSet::new();
    let mut first_sight = |p: &Path| {
        seen.insert(
            std::fs::canonicalize(p)
                .or_else(|_| std::path::absolute(p))
                .unwrap_or_else(|_| p.to_path_buf()),
        )
    };
    for input in inputs {
        if input.is_dir() {
            // Note BC (audit #3): only an output root strictly inside this folder is skipped; in place skips nothing.
            let in_abs = std::path::absolute(input)?;
            let skip_out = is_under(&out_abs, &in_abs) && !is_under(&in_abs, &out_abs);
            let max_depth = if recursive { usize::MAX } else { 1 };
            let walker = walkdir::WalkDir::new(input)
                .min_depth(1)
                .max_depth(max_depth);
            let keep = |e: &walkdir::DirEntry| {
                if !e.file_type().is_dir() {
                    return true;
                }
                e.file_name() != SDC_DIR
                    && !(skip_out
                        && std::path::absolute(e.path()).is_ok_and(|a| is_under(&a, &out_abs)))
            };
            for e in walker.into_iter().filter_entry(keep) {
                let e = match e {
                    Ok(e) => e,
                    // Audit 6: an access-denied folder, a dangling junction or a cloud placeholder skips, never aborts.
                    Err(err) => {
                        // An unreadable input folder itself is still fatal.
                        if err.depth() == 0 {
                            return Err(std::io::Error::other(err));
                        }
                        skipped.push(err.to_string());
                        continue;
                    }
                };
                let p = e.path();
                if !e.file_type().is_file()
                    || (skip_out && is_under(&std::path::absolute(p)?, &out_abs))
                {
                    continue;
                }
                let ext = ext_lower(p);
                if (ext == "dds" || IMAGE_EXTS.contains(&ext.as_str())) && first_sight(p) {
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
            if !first_sight(input) {
                continue;
            }
            files.push(make(input.clone(), PathBuf::from(name), output_root));
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok((files, skipped))
}

/// M6 Track F3b: apply the project's `[inputs]` aliases. A file whose project-relative path matches a key (as a
/// `[files]` key does) takes the alias as its `rel`, `game_path` and output, and keeps its real `rel` in
/// `real_rel`; its `path` (what is decoded) is unchanged. Call it right after discovery, before
/// `canonicalise_outputs`. A refused alias leaves the file as it is (IN-ALIAS-INVALID); a key that matches no file
/// is IN-ALIAS-STALE.
pub fn apply_read_as(
    files: &mut [InputFile],
    root: &Path,
    output_root: &Path,
    inputs: &BTreeMap<String, ReadAs>,
) -> Vec<Warning> {
    let rels: Vec<Option<String>> = files
        .iter()
        .map(|f| crate::overrides::project_rel(root, f))
        .collect();
    // Every real path is taken, and so is every alias once applied: two files never share a name.
    let mut taken: BTreeSet<String> = rels.iter().flatten().map(|r| r.to_lowercase()).collect();
    let mut warnings = vec![];
    for (key, read_as) in inputs {
        let hits: Vec<usize> = (0..files.len())
            .filter(|&i| {
                rels[i]
                    .as_deref()
                    .is_some_and(|r| crate::overrides::path_key_matches(key, r))
            })
            .collect();
        if hits.is_empty() {
            warnings.push(
                Warning::new(
                    Code::InAliasStale,
                    format!("[inputs.\"{key}\"] matches no file; the alias is unused"),
                )
                .with_data(serde_json::json!({ "key": key, "as": read_as.path })),
            );
            continue;
        }
        for i in hits {
            let own = rels[i].as_deref().unwrap_or_default().to_lowercase();
            let applied =
                read_as_target(&files[i], root, output_root, &read_as.path).and_then(|t| {
                    let lower = t.project_rel.to_lowercase();
                    if lower != own && taken.contains(&lower) {
                        Err(format!("{} is already another file's name", t.project_rel))
                    } else {
                        Ok((t, lower))
                    }
                });
            match applied {
                Ok((t, lower)) => {
                    taken.insert(lower);
                    let f = &mut files[i];
                    f.real_rel = Some(std::mem::replace(&mut f.rel, t.rel));
                    f.game_path = t.game_path;
                    f.output = t.output;
                }
                Err(reason) => warnings.push(
                    Warning::new(
                        Code::InAliasInvalid,
                        format!("[inputs.\"{key}\"] ignored: {reason}"),
                    )
                    .with_data(serde_json::json!({
                        "key": key,
                        "as": read_as.path,
                        "reason": reason,
                    })),
                ),
            }
        }
    }
    warnings
}

/// Where an alias puts a file, or why it cannot.
struct ReadAsTarget {
    /// '/'-separated, relative to the project root, with the file's own extension.
    project_rel: String,
    rel: PathBuf,
    game_path: String,
    output: PathBuf,
}

fn read_as_target(
    f: &InputFile,
    root: &Path,
    output_root: &Path,
    alias: &str,
) -> Result<ReadAsTarget, String> {
    use std::path::Component;
    let alias = alias.trim().replace('\\', "/");
    let alias = alias.trim_start_matches("./");
    let as_path = Path::new(alias);
    if alias.is_empty() || alias.ends_with('/') || as_path.file_stem().is_none() {
        return Err("the alias names no file".into());
    }
    if alias.starts_with('/') || as_path.is_absolute() || alias.contains(':') {
        return Err("an alias is relative to the folder holding sdc.toml".into());
    }
    if as_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err("an alias cannot leave the folder holding sdc.toml (..)".into());
    }
    let real_ext = f
        .path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let typed_ext = ext_lower(as_path);
    let with_ext = if typed_ext == real_ext.to_ascii_lowercase() {
        alias.to_string()
    } else if typed_ext == "dds" || IMAGE_EXTS.contains(&typed_ext.as_str()) {
        return Err(format!(
            "an alias keeps the file's own extension (.{real_ext}, not .{typed_ext})"
        ));
    } else {
        format!("{alias}.{real_ext}")
    };
    // The folder the file was found in: its path less the components of its rel.
    let mut found_in = crate::overrides::absolute_or_same(&f.path);
    for _ in f.rel.components() {
        found_in.pop();
    }
    let abs = crate::overrides::absolute_or_same(root).join(&with_ext);
    let rel = abs
        .strip_prefix(&found_in)
        .map_err(|_| "the alias is outside the folder the file was found in".to_string())?
        .to_path_buf();
    Ok(ReadAsTarget {
        project_rel: with_ext,
        game_path: game_path(&abs, &rel),
        output: output_root.join(&rel).with_extension("dds"),
        rel,
    })
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
                VocabRole::PackingInput(_) | VocabRole::NormalLayer(_) => None,
                VocabRole::Colour => Some(format!("{}.dds", split.base)),
                VocabRole::Normal(_) | VocabRole::Emissive | VocabRole::CoatColour => suffixed(),
            },
            (None, None) => None,
        };
        if let Some(n) = name {
            f.output.set_file_name(n);
        }
    }
}
