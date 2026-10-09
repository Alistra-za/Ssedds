//! Texture-set grouping (spec §6.1): inputs sharing an output directory and a base name.
use crate::discover::InputFile;
use std::collections::{BTreeMap, BTreeSet};

/// Spec §6.1 suffix list, longest first so `_msn` wins over `_n` and `_envmask` over `_m`.
pub const SUFFIXES: [&str; 19] = [
    "envmask", "rmaos", "rfaos", "flow", "msn", "cnr", "crn", "em", "bl", "sk", "n", "p", "m", "g",
    "s", "f", "b", "d", "e",
];

/// Splits one recognised suffix off a stem (case-insensitive). The suffix is returned lower-case;
/// a stem that is only a suffix (`_n`) is left whole.
pub fn split_suffix(stem: &str) -> (String, Option<String>) {
    let lower = stem.to_ascii_lowercase();
    for s in SUFFIXES {
        let tail = format!("_{s}");
        if lower.ends_with(&tail) && lower.len() > tail.len() {
            // The tail is ASCII, so the cut falls on a char boundary even in non-ASCII stems.
            return (
                stem[..stem.len() - tail.len()].to_string(),
                Some(s.to_string()),
            );
        }
    }
    (stem.to_string(), None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureSet {
    /// Lower-case output directory + '/' + lower-case base name.
    pub key: String,
    /// Indices into the `files` slice given to `group_sets`.
    pub members: Vec<usize>,
    /// Recognised suffixes present in the set (lower-case; the colour member has none).
    pub suffixes: BTreeSet<String>,
}

fn fold(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Groups inputs into texture sets and reports duplicate groups: two or more inputs that map
/// to one output path, compared case-insensitively.
pub fn group_sets(files: &[InputFile]) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    let mut sets: BTreeMap<String, TextureSet> = BTreeMap::new();
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        outs.entry(fold(&f.output)).or_default().push(i);
        let stem = f
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let (base, suffix) = split_suffix(&stem);
        let dir = f.output.parent().map(fold).unwrap_or_default();
        let key = format!("{dir}/{}", base.to_lowercase());
        let set = sets.entry(key.clone()).or_insert_with(|| TextureSet {
            key,
            members: vec![],
            suffixes: BTreeSet::new(),
        });
        set.members.push(i);
        if let Some(s) = suffix {
            set.suffixes.insert(s);
        }
    }
    let dups = outs.into_values().filter(|v| v.len() > 1).collect();
    (sets.into_values().collect(), dups)
}
