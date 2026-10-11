//! Texture-set grouping (spec §6.1): inputs sharing an output directory and a base name.
use crate::config::{GreenConvention, RawMap};
use crate::discover::{InputFile, InputKind};
use std::collections::{BTreeMap, BTreeSet};

/// Spec §6.1 suffix list, longest first so `_msn` wins over `_n` and `_envmask` over `_m`.
pub const SUFFIXES: [&str; 20] = [
    "envmask", "rmaos", "rfaos", "flow", "msn", "cnr", "crn", "wet", "em", "bl", "sk", "n", "p",
    "m", "g", "s", "f", "b", "d", "e",
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

/// What an exporter suffix stands for (spec §4.4, DECISIONS Note H).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabRole {
    /// The colour member (no suffix): written `<base>.dds`, high confidence for the colour role.
    Colour,
    /// `_n`; `Some` when the term states the green convention (`_normalgl`, `_normaldx`).
    Normal(Option<GreenConvention>),
    /// `_g` (`glow` or `pbr_emissive`).
    Emissive,
    /// A raw single-channel map for channel packing (spec §6.4); never written on its own.
    PackingInput(RawMap),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vocab {
    /// Lower-case, without the leading '_'.
    pub term: &'static str,
    pub role: VocabRole,
}

impl Vocab {
    /// The canonical suffix the file counts as after Discover: none for colour, `n`, `g`, or the packing role id.
    pub fn canonical(&self) -> Option<String> {
        match self.role {
            VocabRole::Colour => None,
            VocabRole::Normal(_) => Some("n".into()),
            VocabRole::Emissive => Some("g".into()),
            VocabRole::PackingInput(r) => Some(r.id()),
        }
    }
}

const fn term(term: &'static str, role: VocabRole) -> Vocab {
    Vocab { term, role }
}

/// Spec §4.4 exporter vocabulary, longest term first so `_base_color` wins over `_color`. None of these is a
/// §6.1 suffix, and none ends in `_<§6.1 suffix>`, so a stem has at most one reading.
pub static VOCABULARY: [Vocab; 31] = {
    use GreenConvention::*;
    use RawMap::*;
    use VocabRole::*;
    [
        term("ambientocclusion", PackingInput(Ao)),
        term("displacement", PackingInput(Height)),
        term("base_color", Colour),
        term("glossiness", PackingInput(Glossiness)),
        term("roughness", PackingInput(Roughness)),
        term("metalness", PackingInput(Metallic)),
        term("basecolor", Colour),
        term("occlusion", PackingInput(Ao)),
        term("normalgl", Normal(Some(Opengl))),
        term("normaldx", Normal(Some(Directx))),
        term("colormap", Colour),
        term("metallic", PackingInput(Metallic)),
        term("specular", PackingInput(Specular)),
        term("emissive", Emissive),
        term("emission", Emissive),
        term("diffuse", Colour),
        term("opacity", PackingInput(Opacity)),
        term("albedo", Colour),
        term("normal", Normal(None)),
        term("height", PackingInput(Height)),
        term("color", Colour),
        term("rough", PackingInput(Roughness)),
        term("metal", PackingInput(Metallic)),
        term("gloss", PackingInput(Glossiness)),
        term("sheen", PackingInput(Fuzz)),
        term("spec", PackingInput(Specular)),
        term("disp", PackingInput(Height)),
        term("fuzz", PackingInput(Fuzz)),
        term("nrm", Normal(None)),
        term("col", Colour),
        term("ao", PackingInput(Ao)),
    ]
};

/// The vocabulary term a stem ends in (case-insensitive) and the base before it. A stem that is only a term
/// (`_normal`) is left alone, as with §6.1 suffixes.
pub fn split_vocab(stem: &str) -> Option<(String, &'static Vocab)> {
    let lower = stem.to_ascii_lowercase();
    VOCABULARY.iter().find_map(|v| {
        let tail = format!("_{}", v.term);
        // The tail is ASCII, so the cut falls on a char boundary even in non-ASCII stems.
        (lower.ends_with(&tail) && lower.len() > tail.len())
            .then(|| (stem[..stem.len() - tail.len()].to_string(), v))
    })
}

/// The vocabulary entry a stem ends in, if any (`split_vocab` without the base).
pub fn vocab_of(stem: &str) -> Option<&'static Vocab> {
    split_vocab(stem).map(|(_, v)| v)
}

/// Whether `term` (lower-case, no '_') is a vocabulary term; preset aliases may not be one (spec §3.7).
pub fn is_vocabulary(term: &str) -> bool {
    VOCABULARY.iter().any(|v| v.term == term)
}

/// Result of `split_suffix_with`: `suffix` is canonical (an alias's target), `alias` the alias matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuffixSplit {
    pub base: String,
    pub suffix: Option<String>,
    pub alias: Option<String>,
}

/// `split_suffix` with preset suffix aliases (spec §3.7, DECISIONS Note G). The stem segment after the
/// last '_' is looked up (lower-case) in `aliases` (alias → canonical) before the vocabulary (§4.4) and
/// the §6.1 list. Aliases contain no '_' and are never a §6.1 suffix or a vocabulary term, so a match is
/// exact and unambiguous. A vocabulary term splits to its canonical suffix (`Vocab::canonical`).
pub fn split_suffix_with(stem: &str, aliases: &BTreeMap<String, String>) -> SuffixSplit {
    if let Some((base, tail)) = stem.rsplit_once('_') {
        let tail = tail.to_ascii_lowercase();
        if let Some(canonical) = aliases.get(&tail).filter(|_| !base.is_empty()) {
            return SuffixSplit {
                base: base.to_string(),
                suffix: Some(canonical.clone()),
                alias: Some(tail),
            };
        }
    }
    // Spec §4.4: exporter vocabulary after aliases, before the §6.1 list.
    if let Some((base, v)) = split_vocab(stem) {
        return SuffixSplit {
            base,
            suffix: v.canonical(),
            alias: None,
        };
    }
    let (base, suffix) = split_suffix(stem);
    SuffixSplit {
        base,
        suffix,
        alias: None,
    }
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
/// to one output path, compared case-insensitively. DDS inputs take no part: they are skipped, not written.
pub fn group_sets(files: &[InputFile]) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    group_sets_with(files, &BTreeMap::new())
}

/// `group_sets` with suffix aliases: an aliased member joins its set under the canonical suffix.
/// Call `discover::canonicalise_outputs` first so alias/canonical collisions are duplicate groups.
pub fn group_sets_with(
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
) -> (Vec<TextureSet>, Vec<Vec<usize>>) {
    let mut sets: BTreeMap<String, TextureSet> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        let stem = f
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let SuffixSplit { base, suffix, .. } = split_suffix_with(&stem, aliases);
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
    (sets.into_values().collect(), duplicate_outputs(files))
}

/// Groups of two or more inputs that map to one output path, compared case-insensitively. DDS inputs take no part:
/// they are skipped, not written. Called again after GUI output paths are applied (M3b), so it is separate.
pub fn duplicate_outputs(files: &[InputFile]) -> Vec<Vec<usize>> {
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if f.kind != InputKind::Dds {
            outs.entry(fold(&f.output)).or_default().push(i);
        }
    }
    outs.into_values().filter(|v| v.len() > 1).collect()
}
