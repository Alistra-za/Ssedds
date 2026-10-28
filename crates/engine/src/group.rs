//! Texture-set grouping (spec §6.1): inputs sharing an output directory and a base name.
use crate::config::{GreenConvention, RawMap};
use crate::discover::{InputFile, InputKind};
use std::collections::{BTreeMap, BTreeSet};

/// Spec §6.1 suffix list, longest first so `_msn` wins over `_n` and `_envmask` over `_m`. `_i` (multilayer inner layer) added in M6 (DECISIONS Note S-shader-6). `_wet` is not on it:
/// it is a suffix only in a cs Advanced skin set ([`join_skin_wet`], final review I3).
pub const SUFFIXES: [&str; 20] = [
    "envmask", "rmaos", "rfaos", "flow", "msn", "cnr", "crn", "em", "bl", "sk", "n", "p", "m", "g",
    "s", "f", "b", "d", "e", "i",
];

/// The marker of a file Tessalith generated (v2 Generate from normal map, DECISIONS Note CO).
pub const GENERATED: &str = "generated";
/// The role tails that may carry the marker: the §6.1 suffix `p` and the vocabulary terms `height`, `ao` and
/// `curves` (Note CO, preflight P10). Only names Tessalith writes carry it.
pub const GENERATED_ROLES: &[&str] = &["p", "height", "ao", "curves"];

/// `stem` without a trailing `_generated` (ASCII case-insensitive) and whether it had one, only when the
/// remainder ends in `_<role>` for a role in [`GENERATED_ROLES`] and is longer than that tail. Otherwise
/// `(stem, false)`, so `rock_generated` and `rock_n_generated` stay whole.
pub fn split_generated(stem: &str) -> (&str, bool) {
    let rest = stem
        .len()
        .checked_sub(GENERATED.len() + 1)
        .filter(|&i| {
            stem.as_bytes()[i] == b'_'
                && stem.as_bytes()[i + 1..].eq_ignore_ascii_case(GENERATED.as_bytes())
        })
        // The tail is ASCII, so the cut falls on a char boundary even in non-ASCII stems.
        .map(|i| &stem[..i]);
    match rest.and_then(|r| r.rsplit_once('_').map(|(b, role)| (r, b, role))) {
        Some((rest, base, role))
            if !base.is_empty() && GENERATED_ROLES.iter().any(|g| role.eq_ignore_ascii_case(g)) =>
        {
            (rest, true)
        }
        _ => (stem, false),
    }
}

/// Whether `stem` names a generated member (`rock_p_generated`): [`split_generated`] strips a marker.
pub fn is_generated(stem: &str) -> bool {
    split_generated(stem).1
}

/// Splits one recognised suffix off a stem (case-insensitive). The suffix is returned lower-case;
/// a stem that is only a suffix (`_n`) is left whole. A generated member splits as its role
/// (`rock_p_generated` → `rock`, `p`).
pub fn split_suffix(stem: &str) -> (String, Option<String>) {
    let stem = split_generated(stem).0;
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
    /// `_coat`: the PBR coat colour, the `_s` member (`pbr_subsurface`; DECISIONS Note S-shader-9).
    CoatColour,
    /// An RNM normal layer or its mask (M6 Track F2); never written on its own.
    NormalLayer(crate::rnm::LayerTerm),
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
            VocabRole::CoatColour => Some("s".into()),
            VocabRole::NormalLayer(t) => Some(t.canonical()),
            VocabRole::PackingInput(r) => Some(r.id()),
        }
    }
}

const fn term(term: &'static str, role: VocabRole) -> Vocab {
    Vocab { term, role }
}

/// Spec §4.4 exporter vocabulary, longest term first so `_base_color` wins over `_color`. None of these is a
/// §6.1 suffix, and none ends in `_<§6.1 suffix>`, so a stem has at most one reading.
/// `_norm` is rnm.py's alias of `_n` (M6 Track F2).
pub static VOCABULARY: [Vocab; 37] = {
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
        term("curvature", PackingInput(Curves)),
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
        term("curves", PackingInput(Curves)),
        term("cavity", PackingInput(Cavity)),
        term("color", Colour),
        term("rough", PackingInput(Roughness)),
        term("metal", PackingInput(Metallic)),
        term("gloss", PackingInput(Glossiness)),
        term("sheen", PackingInput(Fuzz)),
        term("spec", PackingInput(Specular)),
        term("disp", PackingInput(Height)),
        term("fuzz", PackingInput(Fuzz)),
        term("norm", Normal(None)),
        term("coat", CoatColour),
        term("nrm", Normal(None)),
        term("orm", PackingInput(Orm)),
        term("col", Colour),
        term("ao", PackingInput(Ao)),
    ]
};

/// The vocabulary term a stem ends in (case-insensitive) and the base before it. A stem that is only a term
/// (`_normal`) is left alone, as with §6.1 suffixes.
pub fn split_vocab(stem: &str) -> Option<(String, &'static Vocab)> {
    // Note CO: the generated marker comes off before the layer terms too (08e-8).
    let stem = split_generated(stem).0;
    // M6 Track F2: layer terms (longest first) before the exporter vocabulary, so `_detail_mask` is one term.
    if let Some(hit) = crate::rnm::split_layer(stem) {
        return Some(hit);
    }
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
    // DECISIONS DS8: a cube face of an `_e` map (`chrome_e_posx`) is a member of set `chrome` with suffix `e`.
    if let Some((rest, _)) = crate::cube::plan::face_tag(stem) {
        if rest.len() > 2 && rest.to_ascii_lowercase().ends_with("_e") {
            return split_suffix_with(rest, aliases);
        }
    }
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

/// A path compared case-insensitively: '/'-separated and lower-cased.
pub(crate) fn fold(p: &std::path::Path) -> String {
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

/// The skin `_wet` suffix (DECISIONS Note R-skin-1). Not a §6.1 suffix: see [`join_skin_wet`].
pub const SKIN_WET: &str = "wet";

/// The last segment of a set key (its lower-case base name).
fn key_base(key: &str) -> &str {
    key.rsplit_once('/').map_or(key, |(_, b)| b)
}

/// The skin `_wet` member split of `stem` when it belongs to the set `set_key` as one (it was moved there by
/// [`join_skin_wet`]): `x_wet` in set `x`. `None` for every other file.
fn wet_split(stem: &str, set_key: &str) -> Option<SuffixSplit> {
    // Audit 08e-10: Unicode folding, as set keys are built (Note BF).
    wet_tail_split(stem).filter(|s| s.base.to_lowercase() == key_base(set_key).to_lowercase())
}

/// `x_wet` -> base `x`, suffix `wet`, whatever set it is in.
fn wet_tail_split(stem: &str) -> Option<SuffixSplit> {
    let tail = format!("_{SKIN_WET}");
    let lower = stem.to_ascii_lowercase();
    lower.strip_suffix(&tail).filter(|b| !b.is_empty())?;
    Some(SuffixSplit {
        // The tail is ASCII, so the cut falls on a char boundary.
        base: stem[..stem.len() - tail.len()].to_string(),
        suffix: Some(SKIN_WET.to_string()),
        alias: None,
    })
}

/// [`split_suffix_with`] for a member of `set`: as `split_suffix_with`, except that the skin `_wet` member of a set
/// [`join_skin_wet`] joined it to (`x_wet` in set `x`, which then lists the suffix `wet`) splits to its base and
/// the suffix `wet`.
pub fn member_split(
    stem: &str,
    set: &TextureSet,
    aliases: &BTreeMap<String, String>,
) -> SuffixSplit {
    // The key is not checked: a Rename (`x` -> `x2`) keeps the joined `x_wet` (trace-debug 2026-10-16 #3).
    // shortcut: any other suffix-less `*_wet` moved into a set that lists `wet` also reads as its wet map; track
    // joined members by index if that ever matters.
    let s = split_suffix_with(stem, aliases);
    if s.suffix.is_none() && set.suffixes.contains(SKIN_WET) {
        if let Some(w) = wet_tail_split(stem) {
            return w;
        }
    }
    s
}

/// Note DT: a file with a hand-set helper role (`is_helper`) whose name has no recognised suffix (`x_mask`) joins the
/// set of its stem without the last `_segment` (`x`) when that set exists in its folder; otherwise it stays alone.
/// A set left empty is removed.
pub fn join_helpers(
    sets: &mut Vec<TextureSet>,
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
    is_helper: impl Fn(usize) -> bool,
) {
    let mut moves: Vec<(usize, usize, usize)> = vec![];
    for (from, s) in sets.iter().enumerate() {
        for &i in &s.members {
            let stem = files[i]
                .rel
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            if !is_helper(i) || split_suffix_with(&stem, aliases).suffix.is_some() {
                continue;
            }
            // The key's base is the lower-cased stem; cut its last `_segment`, never into the folder.
            let Some((dir, base)) = s.key.rsplit_once('/') else {
                continue;
            };
            let Some((short, _)) = base.rsplit_once('_').filter(|(b, _)| !b.is_empty()) else {
                continue;
            };
            let to_key = format!("{dir}/{short}");
            if let Some(to) = sets.iter().position(|t| t.key == to_key) {
                moves.push((from, to, i));
            }
        }
    }
    for (from, to, i) in moves {
        sets[from].members.retain(|&m| m != i);
        sets[to].members.push(i);
        // Members stay in file order, as `group_sets_with` makes them.
        sets[to].members.sort_unstable();
    }
    sets.retain(|s| !s.members.is_empty());
}

/// Final review I3 (DECISIONS Note R): `_wet` splits off as a suffix only on cs and only when the base is an
/// Advanced skin set. Grouping cannot see the target or the recipe, so `group_sets_with` keeps `x_wet` as its own
/// base (as before `_wet` existed) and this pass, run by the caller on cs before set edits, moves the colour member
/// `x_wet` of set `x_wet` into set `x` when `is_skin(x)`. Members with a suffix of their own (`x_wet_n`) stay. A set
/// left empty is removed.
pub fn join_skin_wet(
    sets: &mut Vec<TextureSet>,
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
    is_skin: impl Fn(&TextureSet) -> bool,
) {
    let tail = format!("_{SKIN_WET}");
    let mut moves: Vec<(usize, usize, Vec<usize>)> = vec![];
    for (from, s) in sets.iter().enumerate() {
        let Some(base_key) = s.key.strip_suffix(&tail) else {
            continue;
        };
        let Some(to) = sets.iter().position(|t| t.key == base_key) else {
            continue;
        };
        let wet: Vec<usize> = s
            .members
            .iter()
            .copied()
            .filter(|&i| {
                let stem = files[i]
                    .rel
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                split_suffix_with(&stem, aliases).suffix.is_none()
                    && wet_split(&stem, base_key).is_some()
            })
            .collect();
        if !wet.is_empty() && is_skin(&sets[to]) {
            moves.push((from, to, wet));
        }
    }
    for (from, to, wet) in moves {
        sets[from].members.retain(|i| !wet.contains(i));
        sets[to].members.extend(wet);
        sets[to].members.sort_unstable();
        sets[to].suffixes.insert(SKIN_WET.to_string());
    }
    sets.retain(|s| !s.members.is_empty());
}

/// Groups of two or more inputs that map to one output path, compared case-insensitively, among the inputs
/// `include` accepts. A DDS takes part only when it is re-encoded (DECISIONS S6).
pub fn duplicate_outputs_with(
    files: &[InputFile],
    include: impl Fn(&InputFile) -> bool,
) -> Vec<Vec<usize>> {
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if include(f) {
            outs.entry(fold(&f.output)).or_default().push(i);
        }
    }
    outs.into_values().filter(|v| v.len() > 1).collect()
}

/// `duplicate_outputs_with` over every image input (DDS inputs are skipped, not written).
pub fn duplicate_outputs(files: &[InputFile]) -> Vec<Vec<usize>> {
    duplicate_outputs_with(files, |f| f.kind != InputKind::Dds)
}

/// Whether a real member `real_stem` of kind `real_kind` replaces the generated member `generated_stem` of the same
/// set (Note CO, the M9 fold). Slot files (`_p`, `_m`, `_rmaos`, `_rfaos`) count as an image or a DDS; raw maps only
/// as an image, as `pack::plan::raw_role` reads them. A raw height is no rival of a generated `_p` on `vanilla`,
/// where nothing is built from it. An `_rmaos`/`_rfaos` image is no rival of a generated `_ao` on `cs`: pack planning
/// decides on its B (Note CO103). A generated stem is never a rival. Callers pass `real_stem` canonical
/// (`<base>_<suffix>` after `split_suffix_with` with the preset aliases), as [`generated_losers`] does: an aliased
/// stem passed raw is not recognised.
pub fn real_rival(
    generated_stem: &str,
    real_stem: &str,
    real_kind: InputKind,
    target: crate::config::Target,
) -> bool {
    let (rest, generated) = split_generated(generated_stem);
    if !generated || is_generated(real_stem) {
        return false;
    }
    let role = rest
        .rsplit_once('_')
        .map_or("", |(_, r)| r)
        .to_ascii_lowercase();
    let real = split_suffix_with(real_stem, &BTreeMap::new()).suffix;
    let image = real_kind == InputKind::Image;
    match (role.as_str(), real.as_deref()) {
        ("p", Some("p")) | ("height", Some("p" | "m")) => true,
        // Note CO103: on cs, pack planning reads a pre-packed image's B by content instead.
        ("ao", Some("rmaos" | "rfaos")) => !image || target != crate::config::Target::Cs,
        ("p", Some("height")) => image && target == crate::config::Target::Cs,
        ("height", Some("height")) | ("ao", Some("ao" | "orm")) | ("curves", Some("curves")) => {
            image
        }
        _ => false,
    }
}

/// (generated member, its first real rival) pairs within each set, by [`real_rival`]. A DDS that is an earlier export
/// in place of the generated member's own output or of a packed output it feeds (`rock_p`, `rock_m`, `rock_rmaos`
/// next to its output, or the set's packed output where `packed` (the explicit `output_paths`, by set key and suffix)
/// moved it; `dds::same_file`) is not a rival (Note CO8, CO10, CO102).
pub fn generated_losers(
    sets: &[TextureSet],
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
    target: crate::config::Target,
    packed: &BTreeMap<(String, String), std::path::PathBuf>,
) -> Vec<(usize, usize)> {
    let stem = |i: usize| {
        files[i]
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    };
    // Canonical stems, so an aliased real map counts under its canonical suffix.
    let canonical = |i: usize| {
        let s = split_suffix_with(&stem(i), aliases);
        s.suffix
            .map_or(stem(i).into_owned(), |x| format!("{}_{x}", s.base))
    };
    let mut out = vec![];
    for set in sets {
        for &g in set.members.iter().filter(|&&g| is_generated(&stem(g))) {
            let rival = set.members.iter().copied().find(|&r| {
                r != g
                    && !is_generated(&stem(r))
                    && real_rival(&stem(g), &canonical(r), files[r].kind, target)
                    && !(files[r].kind == InputKind::Dds
                        && (crate::dds::same_file(
                            &files[r].path,
                            &files[g]
                                .output
                                .with_file_name(files[r].path.file_name().unwrap_or_default()),
                        ) || packed
                            .iter()
                            .filter(|((set_key, _), _)| *set_key == set.key)
                            .any(|(_, out)| crate::dds::same_file(&files[r].path, out))))
            });
            out.extend(rival.map(|r| (g, r)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// Audit 08e-10: set keys fold Unicode (Note BF), so the skin `_wet` join must too.
    #[test]
    fn wet_split_folds_like_set_keys() {
        assert!(super::wet_split("Ärmel_wet", "textures/a/ärmel").is_some());
        assert!(super::wet_split("Ärmel_wet", "textures/a/other").is_none());
    }
}
