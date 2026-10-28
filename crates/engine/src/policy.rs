//! Resolve (spec §5.2): merge preset, config, rules, analysis and overrides into an immutable ResolvedJob.
//!
//! Precedence (spec §8, lowest first): built-in (`sdc-base`) → preset chain → user config → project config →
//! rule matches (every matching rule of a source, in file order, field by field; sources in the same order;
//! DECISIONS AP10) → analysis facts → the project config's
//! `[files]` entries (DECISIONS N2) → `--set` (unscoped, then sub-type-scoped, then file-scoped, each in
//! command-line order).
use crate::analyse::Analysis;
use crate::codes::{Code, Severity, Warning};
use crate::config::*;
use crate::dds::{resolve_header, ResolvedHeader};
use crate::discover::InputFile;
use crate::group::{vocab_of, TextureSet, Vocab, VocabRole};
use crate::image::SourceInfo;
use crate::overrides::{to_patch, FileScopes, Scope, SetOverride};
use crate::pack::plan::set_is_advanced_skin;
pub use crate::presets::Provenance;
use crate::presets::*;
use crate::suggest::*;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum AcceptSuggestions {
    None,
    High,
}

pub struct ResolveCtx<'a> {
    pub target: Target,
    pub registry: &'a PresetRegistry,
    /// The batch preset.
    pub preset: &'a str,
    pub user_config: Option<&'a ConfigFile>,
    pub project_config: Option<&'a ConfigFile>,
    /// `--set` overrides in command-line order.
    pub overrides: &'a [SetOverride],
    pub accept: AcceptSuggestions,
    /// Codes allowed with `--allow`.
    pub allow: &'a BTreeSet<Code>,
    /// The `[files]` layer and per-file acknowledgements (DECISIONS N2).
    pub files: &'a FileScopes,
    /// M6 (DECISIONS Note S): set key → the set's shader pick. Empty during pass 1 (`NO_SHADERS`).
    pub shaders: &'a BTreeMap<String, crate::shader::ShaderPick>,
    /// M6 Track E (DECISIONS Note U5): `shader::colour_mesh_key` → the alpha property of each scanned shape that uses
    /// the colour member (`BatchRequest::mesh_alpha`). Empty (`NO_MESH_ALPHA`) = no evidence.
    pub mesh_alpha: &'a BTreeMap<String, Vec<crate::shader::MeshAlpha>>,
    /// M5 (DECISIONS Note NV3): the batch's NVTT probe, run on the first profile on `encoder.backend = "nvtt"`.
    /// `None` = not probed (tests, `inspect`): such a profile reports ENC-NVTT-MISSING.
    pub nvtt: Option<&'a crate::nvtt::NvttCell>,
}

/// For `ResolveCtx` literals made before the shaders are picked (pass 1, tests, `inspect`).
pub static NO_SHADERS: BTreeMap<String, crate::shader::ShaderPick> = BTreeMap::new();

/// For `ResolveCtx` literals with no mesh evidence (tests).
pub static NO_MESH_ALPHA: BTreeMap<String, Vec<crate::shader::MeshAlpha>> = BTreeMap::new();

pub struct FileFacts<'a> {
    pub input: &'a InputFile,
    /// Canonical suffix (an alias already mapped to its target, spec §3.7).
    pub suffix: Option<&'a str>,
    /// The suffix alias the file name matched (`group::split_suffix_with`), if any.
    pub alias: Option<&'a str>,
    pub set: &'a TextureSet,
    pub analysis: &'a Analysis,
    pub info: &'a SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtypeDecision {
    pub subtype: Subtype,
    pub provenance: Provenance,
    /// True only for an explicit `--set`, a rule, or a high-confidence suggestion accepted with
    /// `--accept-suggestions high`.
    pub decided: bool,
    pub suggestions: Vec<Suggestion>,
}

/// A suffix alias in use for one file (spec §3.7, DECISIONS Note G).
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct AliasUse {
    pub alias: String,
    pub canonical: String,
    pub preset: String,
}

/// `(alias, canonical, preset id)` when the file matched an alias the registry knows.
fn alias_parts<'a>(ctx: &ResolveCtx<'a>, f: &FileFacts) -> Option<(String, &'a str, &'a str)> {
    let alias = f.alias?.to_ascii_lowercase();
    let (canonical, preset) = ctx.registry.alias_for(&alias)?;
    Some((alias, canonical, preset))
}

/// The preset whose chain resolves this file: the alias preset for an aliased file, else the batch preset.
pub fn preset_for<'a>(ctx: &ResolveCtx<'a>, f: &FileFacts) -> &'a str {
    alias_parts(ctx, f).map_or(ctx.preset, |(_, _, p)| p)
}

fn alias_use(ctx: &ResolveCtx, f: &FileFacts) -> Option<AliasUse> {
    alias_parts(ctx, f).map(|(alias, c, p)| AliasUse {
        alias,
        canonical: c.to_string(),
        preset: p.to_string(),
    })
}

/// The file's preset chain, root base first: the batch preset's, or for an aliased file the alias preset's
/// (`PresetRegistry::alias_chain`: an alias preset without `base` sits on the target's quality preset).
fn preset_chain<'a>(
    ctx: &ResolveCtx<'a>,
    f: &FileFacts,
) -> Result<Vec<&'a PresetFile>, PresetError> {
    match alias_parts(ctx, f) {
        Some((_, _, p)) => ctx.registry.alias_chain(p, ctx.target),
        None => ctx.registry.chain(ctx.preset),
    }
}

/// The exporter vocabulary term the file name ends in (spec §4.4), if any. Aliases are matched first and never
/// equal a term, so an aliased file has none.
pub fn vocab_for(f: &FileFacts) -> Option<&'static Vocab> {
    if f.alias.is_some() {
        return None;
    }
    vocab_of(
        &f.input
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
    )
}

/// Sub-types that read `channels.green_convention` (spec §3.2): every Normal sub-type except `msn`.
fn takes_green(st: Subtype) -> bool {
    st.category() == Category::Normal && st != Subtype::Msn
}

/// Case-insensitive glob (Unicode simple lower-casing of the pattern and both paths) over the game path or the
/// input-relative path. A glob without '/' also matches the file name alone, so `file:*.png` and
/// `file:wood01.png` work in any folder.
fn glob_match_input(glob: &str, input: &InputFile) -> bool {
    thread_local! {
        // shortcut: never evicted; globs come from presets, configs and --set (dozens), so bound it if they ever come
        // from data.
        static GLOBS: std::cell::RefCell<std::collections::HashMap<String, Option<globset::GlobMatcher>>> =
            Default::default();
    }
    let rel = input
        .rel
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let name = rel.rsplit('/').next().unwrap_or_default();
    let game = input.game_path.to_lowercase();
    GLOBS.with_borrow_mut(|c| {
        if !c.contains_key(glob) {
            c.insert(glob.to_owned(), compile_glob(glob).ok());
        }
        let Some(m) = &c[glob] else { return false };
        m.is_match(&game) || m.is_match(&rel) || (!glob.contains('/') && m.is_match(name))
    })
}

/// The matcher [`glob_match_input`] uses for `glob`, or why it does not compile. Patterns are checked with it where
/// they enter (`parse_set`, `check_preset`, `check_up_front`; audit #7, Note BC), so a typo is an error, never a
/// glob that matches nothing.
pub(crate) fn compile_glob(glob: &str) -> Result<globset::GlobMatcher, String> {
    globset::GlobBuilder::new(&glob.to_lowercase())
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("bad glob '{glob}': {e}"))
}

fn glob_match(glob: &str, f: &FileFacts) -> bool {
    glob_match_input(glob, f.input)
}

/// `st` is the sub-type being resolved, or `None` while it is being decided: a rule with `match.subtypes` then never
/// matches (DECISIONS Note T2). `alpha` is the alpha content `match.alpha` is compared with (usually
/// `f.analysis.alpha`); `None` = a rule that matches on `alpha` never matches.
pub fn rule_matches(
    m: &RuleMatch,
    f: &FileFacts,
    st: Option<Subtype>,
    alpha: Option<AlphaContent>,
) -> bool {
    m.glob.as_deref().is_none_or(|g| glob_match(g, f))
        && m.suffix
            .as_deref()
            .is_none_or(|s| f.suffix.unwrap_or("").eq_ignore_ascii_case(s))
        && m.set_has
            .as_deref()
            .is_none_or(|s| f.set.suffixes.contains(&s.to_ascii_lowercase()))
        && m.alpha.is_none_or(|a| Some(a) == alpha)
        && (m.subtypes.is_empty() || st.is_some_and(|s| m.subtypes.contains(&s)))
}

/// Rule sources in precedence order: the file's preset chain (base first; `preset_chain`), user config,
/// project config.
fn rule_sources<'a>(ctx: &ResolveCtx<'a>, chain: &[&'a PresetFile]) -> Vec<(String, &'a [Rule])> {
    let mut v: Vec<(String, &'a [Rule])> = vec![];
    for p in chain {
        v.push((p.id.clone(), &p.rules));
    }
    if let Some(c) = ctx.user_config {
        v.push(("user".into(), &c.rules));
    }
    if let Some(c) = ctx.project_config {
        v.push(("project".into(), &c.rules));
    }
    v
}

/// Every rule of one source that matches, in file order: they layer field by field, so a later rule overrides only
/// the fields it sets (owner ruling, DECISIONS AP10). While layering sub-type `st`, a rule that names another sub-type
/// (`Rule::subtype`) does not match, so a `--set`/`[files]` sub-type drops it (carried fix 3, Note T10). `alpha` as
/// in [`rule_matches`].
fn matching<'a>(
    rules: &'a [Rule],
    f: &'a FileFacts,
    st: Option<Subtype>,
    alpha: Option<AlphaContent>,
) -> impl Iterator<Item = (usize, &'a Rule)> {
    rules.iter().enumerate().filter(move |(_, r)| {
        st.is_none_or(|s| r.subtype.is_none_or(|x| x == s))
            && rule_matches(&r.matcher, f, st, alpha)
    })
}

/// Whether `code` is acknowledged for this file: batch-wide `--allow`, or the file's own `allow`.
pub fn allowed(ctx: &ResolveCtx, f: &FileFacts, code: Code) -> bool {
    if ctx.allow.contains(&code) {
        return true;
    }
    if ctx.files.allow.is_empty() {
        return false;
    }
    ctx.files.forms_of(f.input).is_some_and(|forms| {
        ctx.files
            .allow
            .iter()
            .any(|(k, codes)| codes.contains(&code) && forms.matches(k))
    })
}

/// A file's `[files]` match forms, fetched at most once per pass over the overrides (DECISIONS AV3).
type LazyForms<'a> = std::cell::OnceCell<Option<std::borrow::Cow<'a, crate::overrides::KeyForms>>>;

fn path_matches<'a>(
    ctx: &ResolveCtx<'a>,
    forms: &LazyForms<'a>,
    input: &InputFile,
    key: &str,
) -> bool {
    forms
        .get_or_init(|| ctx.files.forms_of(input))
        .as_ref()
        .is_some_and(|m| m.matches(key))
}

fn override_applies<'a>(
    ctx: &ResolveCtx<'a>,
    o: &SetOverride,
    st: Option<Subtype>,
    f: &FileFacts,
    forms: &LazyForms<'a>,
) -> bool {
    match &o.scope {
        Scope::All => true,
        Scope::Subtype(s) => Some(*s) == st,
        Scope::File(g) => glob_match(g, f),
        Scope::Path(k) => path_matches(ctx, forms, f.input, k),
        Scope::Set(_) => false,
    }
}

/// A `[files]` entry is `project_file`; a command-line `--set` is `user`.
fn override_provenance(o: &SetOverride) -> Provenance {
    match o.scope {
        Scope::Path(_) => Provenance::ProjectFile,
        _ => Provenance::User,
    }
}

/// Overrides, lowest precedence first: the `[files]` layer, then the command line (unscoped, sub-type, file glob,
/// path), each group in its own order (the sort is stable). DECISIONS N2: the command line always wins.
fn ranked_overrides<'a>(ctx: &ResolveCtx<'a>) -> Vec<&'a SetOverride> {
    let rank = |o: &SetOverride| match o.scope {
        Scope::All => 0,
        Scope::Subtype(_) => 1,
        Scope::File(_) => 2,
        Scope::Path(_) => 3,
        Scope::Set(_) => 4,
    };
    let mut cli: Vec<&SetOverride> = ctx.overrides.iter().collect();
    cli.sort_by_key(|o| rank(o));
    ctx.files.overrides.iter().chain(cli).collect()
}

/// DECISIONS S6: whether a DDS input is re-encoded. The highest-precedence `reencode` override that reaches the
/// file decides: `[files]`, then unscoped, file-glob and path `--set` (a sub-type scope is refused at parse time).
pub fn reencode_requested(ctx: &ResolveCtx, input: &InputFile) -> bool {
    let forms = LazyForms::new();
    ranked_overrides(ctx)
        .into_iter()
        .rev()
        .filter(|o| o.field == crate::overrides::REENCODE_FIELD)
        .find(|o| match &o.scope {
            Scope::All => true,
            Scope::File(g) => glob_match_input(g, input),
            Scope::Path(k) => path_matches(ctx, &forms, input, k),
            Scope::Subtype(_) | Scope::Set(_) => false,
        })
        .is_some_and(|o| o.value == Value::Bool(true))
}
/// Whether a `[files]` entry or file-scoped `--set` names this input's sub-type (any value). Trace-debug 2026-10-16
/// #4: a raw map given one is an ordinary file (spec §4.4) and names no packing role.
pub fn subtype_requested(ctx: &ResolveCtx, input: &InputFile) -> bool {
    let forms = LazyForms::new();
    ranked_overrides(ctx).into_iter().any(|o| {
        o.field == "subtype"
            && match &o.scope {
                Scope::All => true,
                Scope::File(g) => glob_match_input(g, input),
                Scope::Path(k) => path_matches(ctx, &forms, input, k),
                Scope::Subtype(_) | Scope::Set(_) => false,
            }
    })
}
/// M6 Track F2 (DECISIONS Note W3): the `rnm.target` a layer file asked for, by the highest-precedence override that
/// reaches it (`[files]`, then unscoped, file-glob and path `--set`); `None` = the default from its name.
pub fn rnm_target_requested(ctx: &ResolveCtx, input: &InputFile) -> Option<RnmTarget> {
    let forms = LazyForms::new();
    ranked_overrides(ctx)
        .into_iter()
        .rev()
        .filter(|o| o.field == crate::overrides::RNM_TARGET_FIELD)
        .find(|o| match &o.scope {
            Scope::All => true,
            Scope::File(g) => glob_match_input(g, input),
            Scope::Path(k) => path_matches(ctx, &forms, input, k),
            Scope::Subtype(_) | Scope::Set(_) => false,
        })
        .and_then(|o| crate::overrides::rnm_target_value(&o.value))
}

pub fn decide_subtype(ctx: &ResolveCtx, f: &FileFacts) -> SubtypeDecision {
    // An invalid chain is reported by `layered_profile`; here it only contributes no rules.
    let chain = preset_chain(ctx, f).unwrap_or_default();
    let mut suggestions = suggest(&SuggestContext {
        target: ctx.target,
        suffix: f.suffix,
        game_path: &f.input.game_path,
        set_suffixes: &f.set.suffixes,
        alpha: f.analysis.alpha,
        is_float: f.info.is_float,
        advanced_skin: set_is_advanced_skin(ctx, f.set, f.input),
    });
    if let Some((alias, canonical, preset)) = alias_parts(ctx, f) {
        for s in &mut suggestions {
            s.reason = format!(
                "suffix alias _{alias} → _{canonical} (preset {preset}); {}",
                s.reason
            );
        }
        // Spec §3.7: an alias declares the suffix; `_s` keeps its ranked readings (§4.3 item 2).
        if canonical != "s" {
            if let Some(top) = suggestions.first_mut() {
                top.confidence = Confidence::High;
            }
        }
    }
    if let Some(v) = vocab_for(f) {
        apply_vocabulary(&mut suggestions, v);
    }
    // --set subtype (unscoped or file-scoped): the highest-precedence one wins
    let forms = LazyForms::new();
    let set_subtype = ranked_overrides(ctx)
        .into_iter()
        .rev()
        .find(|o| o.field == "subtype" && override_applies(ctx, o, None, f, &forms));
    if let Some(o) = set_subtype {
        if let Ok(st) = serde_json::from_value::<Subtype>(o.value.clone()) {
            return SubtypeDecision {
                subtype: st,
                provenance: override_provenance(o),
                decided: true,
                suggestions,
            };
        }
    }
    // rules: highest-precedence source with a matching rule that names a sub-type. The sub-type is decided by the
    // first matching rule of a source, as before; only the field overrides layer (AP10)
    for (src, rules) in rule_sources(ctx, &chain).into_iter().rev() {
        if let Some((i, r)) = matching(rules, f, None, Some(f.analysis.alpha)).next() {
            if let Some(st) = r.subtype {
                return SubtypeDecision {
                    subtype: st,
                    provenance: Provenance::Rule(format!("{src}#{i}")),
                    decided: true,
                    suggestions,
                };
            }
        }
    }
    // DECISIONS DS7: a cube DDS can only be a cubemap (an explicit sub-type above gets CFG-INVALID in `check_profile`).
    if f.analysis.dds_cube {
        return SubtypeDecision {
            subtype: Subtype::Cubemap,
            provenance: Provenance::AnalysisFact("cube-dds".into()),
            decided: true,
            suggestions,
        };
    }
    // S-shader-4: the set's shader re-types a member (eye `_n` → msn, landscape colour → landscape_diffuse).
    if let Some(p) = ctx.shaders.get(&f.set.key) {
        let id = p.shader.id();
        if let Some((st, why)) = suggestions
            .first()
            .and_then(|s| crate::shader::member_subtype(p.shader, s.subtype))
        {
            if p.provenance == crate::shader::ShaderOrigin::Set {
                return SubtypeDecision {
                    subtype: st,
                    provenance: Provenance::Shader(id.into()),
                    decided: true,
                    suggestions,
                };
            }
            suggestions.insert(
                0,
                Suggestion {
                    subtype: st,
                    confidence: Confidence::High,
                    reason: format!("{id} shader ({}): {why}", p.provenance),
                },
            );
        }
    }
    match suggestions.first() {
        Some(s) => {
            let decided = ctx.accept == AcceptSuggestions::High && s.confidence == Confidence::High;
            SubtypeDecision {
                subtype: s.subtype,
                provenance: Provenance::Suggestion,
                decided,
                suggestions,
            }
        }
        None => SubtypeDecision {
            subtype: Subtype::GenericData,
            provenance: Provenance::Suggestion,
            decided: false,
            suggestions,
        },
    }
}

/// How specifically a layer patch addresses this file, for `backend_follows_format`.
#[derive(Debug, Clone, Copy)]
struct Reach {
    /// 0 = every sub-type (`defaults` tables, unscoped `--set`), 1 = this sub-type, 2 = this file.
    level: u8,
    /// Whether a backend set here is kept against the format: only sub-type- or file-scoped `--set` and rules.
    /// Preset and config tables never pin a backend unless the same table also sets the format.
    pins: bool,
}

const GENERIC: Reach = Reach {
    level: 0,
    pins: false,
};
const SUBTYPE_TABLE: Reach = Reach {
    level: 1,
    pins: false,
};
const SUBTYPE_SET: Reach = Reach {
    level: 1,
    pins: true,
};
const FILE: Reach = Reach {
    level: 2,
    pins: true,
};

/// Which patch last set `format` and `encoder.backend`, and how specifically; how specifically `mips.filter` was set.
#[derive(Debug, Default)]
struct FormatSources {
    patches: usize,
    format: Option<(Reach, usize)>,
    backend: Option<(Reach, usize)>,
    filter: Option<Reach>,
}

fn apply(
    doc: &mut Value,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &mut FormatSources,
    patch: &Value,
    source: Provenance,
    reach: Reach,
) {
    let mut paths = vec![];
    leaf_paths(patch, "", &mut paths);
    merge_json(doc, patch);
    let id = sources.patches;
    sources.patches += 1;
    for p in paths {
        match p.as_str() {
            "format" => sources.format = Some((reach, id)),
            "encoder.backend" => sources.backend = Some((reach, id)),
            "mips.filter" => sources.filter = Some(reach),
            _ => {}
        }
        prov.insert(p, source.clone());
    }
}

fn config_patches(c: &ConfigFile, st: Subtype) -> Vec<(Reach, Value)> {
    let mut v = vec![(GENERIC, c.defaults.clone())];
    if let Some(s) = c.subtypes.get(&st) {
        v.push((SUBTYPE_TABLE, s.clone()));
    }
    v
}

/// Whether `alpha.policy` was chosen by an explicit layer (spec §5.2): a user or project preset, a config, a
/// rule or `--set`. The hidden base and the built-in presets only carry defaults.
fn alpha_policy_explicit(ctx: &ResolveCtx, prov: &BTreeMap<String, Provenance>) -> bool {
    field_explicit(ctx, prov, "alpha.policy")
}

/// Whether `field` was chosen by an explicit layer (see [`alpha_policy_explicit`]).
fn field_explicit(ctx: &ResolveCtx, prov: &BTreeMap<String, Provenance>, field: &str) -> bool {
    match prov.get(field) {
        None | Some(Provenance::Builtin) => false,
        Some(Provenance::Preset(id)) => !ctx.registry.is_builtin(id),
        Some(_) => true,
    }
}

/// The profile for sub-type `st` with the source of every field (dotted path → provenance).
///
/// Analysis-derived `alpha.policy = remove` (no alpha, or constant 255 where alpha is optional) applies only
/// where no preset, config or rule set the policy, and regardless of `--accept-suggestions`: it is a fact about
/// the source, not a guess.
pub fn layered_profile(
    ctx: &ResolveCtx,
    f: &FileFacts,
    st: Subtype,
) -> Result<(Profile, BTreeMap<String, Provenance>), PresetError> {
    layered_profile_with(ctx, f, st, false).map(|(p, prov, _)| (p, prov))
}

/// `layered_profile` for a packed output whose image does not exist yet (DECISIONS Note M6): `f.analysis` is a
/// placeholder, so rules that match on `alpha` (whatever the value) are ignored.
pub fn layered_profile_placeholder(
    ctx: &ResolveCtx,
    f: &FileFacts,
    st: Subtype,
) -> Result<(Profile, BTreeMap<String, Provenance>), PresetError> {
    layered_profile_with(ctx, f, st, true).map(|(p, prov, _)| (p, prov))
}

/// The profile, its provenance and the filter dropped by `unscoped_filter_falls_back` (with why), if any.
type Layered = (
    Profile,
    BTreeMap<String, Provenance>,
    Option<(MipFilter, &'static str)>,
);

fn layered_profile_with(
    ctx: &ResolveCtx,
    f: &FileFacts,
    st: Subtype,
    ignore_alpha_rules: bool,
) -> Result<Layered, PresetError> {
    // The alias preset's chain for an aliased file (spec §3.7), else the batch preset's.
    let chain = preset_chain(ctx, f)?;
    let forms = LazyForms::new();
    let mut doc = Value::Object(Map::new());
    let mut prov = BTreeMap::new();
    let mut sources = FormatSources::default();
    // `layers_of` returns every non-null `defaults` table of the chain first, then the `subtypes.<st>` tables.
    let generic = chain.iter().filter(|p| !p.defaults.is_null()).count();
    for (i, (source, patch)) in PresetRegistry::layers_of(&chain, st)
        .into_iter()
        .enumerate()
    {
        let reach = if i < generic { GENERIC } else { SUBTYPE_TABLE };
        apply(&mut doc, &mut prov, &mut sources, &patch, source, reach);
    }
    for (cfg, source) in [
        (ctx.user_config, Provenance::UserConfig),
        (ctx.project_config, Provenance::ProjectConfig),
    ] {
        if let Some(c) = cfg {
            for (reach, patch) in config_patches(c, st) {
                apply(
                    &mut doc,
                    &mut prov,
                    &mut sources,
                    &patch,
                    source.clone(),
                    reach,
                );
            }
        }
    }
    // M6 Track E (spec §4.3, DECISIONS Note U5): what the meshes that use this colour member say about its alpha.
    let mesh = st
        .alpha_from_mesh()
        .then(|| crate::shader::colour_mesh_key(&f.input.game_path, &f.input.output, Some(st)))
        .flatten()
        .and_then(|k| ctx.mesh_alpha.get(&k))
        .and_then(|shapes| crate::shader::mesh_alpha_policy(shapes));
    // Owner O1: a varying alpha the meshes drop (and no explicit layer keeps) is opaque colour, so rules match it as
    // alpha `none` (Compact's BC1 rules). Known before the rules: the mesh layer itself is applied below. A rule that
    // sets the policy, matched on the source's real alpha, is explicit and keeps the mesh out (fix round 1).
    let real_alpha = (!ignore_alpha_rules).then_some(f.analysis.alpha);
    let mesh_remove = f.analysis.alpha == AlphaContent::Varying
        && matches!(mesh, Some((AlphaPolicy::Remove, _)))
        && !alpha_policy_explicit(ctx, &prov)
        && !ranked_overrides(ctx)
            .into_iter()
            .any(|o| o.field == "alpha.policy" && override_applies(ctx, o, Some(st), f, &forms))
        && !rule_sources(ctx, &chain).into_iter().any(|(_, rules)| {
            matching(rules, f, Some(st), real_alpha)
                .any(|(_, r)| r.overrides.pointer("/alpha/policy").is_some())
        });
    let rule_alpha = real_alpha.map(|a| if mesh_remove { AlphaContent::None } else { a });
    for (src, rules) in rule_sources(ctx, &chain) {
        for (i, r) in matching(rules, f, Some(st), rule_alpha) {
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &r.overrides,
                Provenance::Rule(format!("{src}#{i}")),
                FILE,
            );
        }
    }
    if !alpha_policy_explicit(ctx, &prov) {
        if let Some((policy, Confidence::High, _)) =
            suggest_alpha(st, f.analysis.alpha, near_binary(f.analysis))
        {
            let value = serde_json::to_value(policy).expect("AlphaPolicy serialises");
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch("alpha.policy", &value),
                Provenance::Analysis,
                GENERIC,
            );
        }
    }
    // DECISIONS AH2: complex material parallax defaults on only for an alpha that really varies (a height map). Any
    // constant alpha (0, 128, 255) and Note L's near-opaque noise (classed Constant255) is off.
    if st == Subtype::ComplexMaterial && !field_explicit(ctx, &prov, "alpha.parallax") {
        let a = &f.analysis;
        let (on, fact) = match a.alpha {
            AlphaContent::None => (false, "no-alpha"),
            AlphaContent::Varying if a.channels[3].min < a.channels[3].max => {
                (true, "varying-alpha")
            }
            _ => (false, "constant-alpha"),
        };
        apply(
            &mut doc,
            &mut prov,
            &mut sources,
            &to_patch("alpha.parallax", &json!(on)),
            Provenance::AnalysisFact(fact.into()),
            GENERIC,
        );
    }
    if st == Subtype::SkinWet {
        skin_wet_form(ctx, f, &mut doc, &mut prov, &mut sources);
    }
    // M6 Track E (spec §4.3, DECISIONS Note U7): the mesh evidence, only where no explicit layer and no analysis fact
    // (a source without varying alpha) chose the policy; it is explicit itself, so the shader default below never
    // overwrites it and the alpha question is not asked.
    if let Some((policy, threshold)) = mesh.filter(|_| !alpha_policy_explicit(ctx, &prov)) {
        let value = serde_json::to_value(policy).expect("AlphaPolicy serialises");
        apply(
            &mut doc,
            &mut prov,
            &mut sources,
            &to_patch("alpha.policy", &value),
            Provenance::MeshAlpha,
            GENERIC,
        );
        if let Some(t) = threshold.filter(|_| !field_explicit(ctx, &prov, "alpha.threshold")) {
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch("alpha.threshold", &json!(t)),
                Provenance::MeshAlpha,
                GENERIC,
            );
        }
    }
    // X12: complex grass is cutout (owner's mods: 159 of 193 loose grass diffuses); the bottom half stays data
    // (grass::half_profile). Mesh evidence and any explicit layer above already chose the policy.
    if st == Subtype::ComplexGrass
        && f.analysis.alpha == AlphaContent::Varying
        && !alpha_policy_explicit(ctx, &prov)
    {
        apply(
            &mut doc,
            &mut prov,
            &mut sources,
            &to_patch("alpha.policy", &json!(AlphaPolicy::Cutout)),
            Provenance::Builtin,
            GENERIC,
        );
    }
    if let Some(p) = ctx.shaders.get(&f.set.key) {
        shader_layer(ctx, p.shader, st, &mut doc, &mut prov, &mut sources);
    }
    // Spec §4.4: `_normalgl`/`_normaldx` state the green convention, a fact at the analysis position (--set wins).
    if let Some(Vocab {
        term,
        role: VocabRole::Normal(Some(g)),
    }) = vocab_for(f)
    {
        if takes_green(st) {
            let value = serde_json::to_value(g).expect("GreenConvention serialises");
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch("channels.green_convention", &value),
                Provenance::Suffix(term.to_string()),
                GENERIC,
            );
        }
    }
    for o in ranked_overrides(ctx)
        .into_iter()
        .filter(|o| o.field != "subtype" && !crate::overrides::is_selector(&o.field))
    {
        if override_applies(ctx, o, Some(st), f, &forms) {
            let reach = match o.scope {
                Scope::All => GENERIC,
                Scope::Subtype(_) => SUBTYPE_SET,
                Scope::File(_) | Scope::Path(_) => FILE,
                Scope::Set(_) => unreachable!("set scopes never apply to a file"),
            };
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch(&o.field, &o.value),
                override_provenance(o),
                reach,
            );
        }
    }
    if st == Subtype::Cubemap {
        cube_style_layer(ctx, &mut doc, &mut prov, &mut sources);
    }
    let mut profile = finish_profile(doc, ctx.target, st)?;
    // The skin_wet height form is a grayscale map: R is taken and copied to RGB in Preprocess.
    profile.category = category_of(st, profile.format, f.analysis.grayscale);
    backend_follows_format(&mut profile, &mut prov, &sources);
    emissive_hdr_choice(ctx, &mut profile, &mut prov, &sources, f);
    float_source_is_linear(&mut profile, &mut prov, f);
    // Spec §3.2, §5.4: a stated OpenGL convention is flipped to DirectX in Preprocess (Task 28 reads flip_green). An
    // explicit `flip_green` wins; `resolve_decided` reports a contradiction as CFG-GREEN-CONFLICT (carried fix 4).
    if takes_green(st)
        && profile.channels.green_convention == Some(GreenConvention::Opengl)
        && !field_explicit(ctx, &prov, "channels.flip_green")
    {
        profile.channels.flip_green = true;
        if let Some(src) = prov.get("channels.green_convention").cloned() {
            prov.insert("channels.flip_green".into(), src);
        }
    }
    // Trace-debug 2026-10-16 #5: a BC5 or RG8 source decodes with B = 0 (Z = -1), which the clamp in Preprocess would
    // turn into a sideways vector. Rebuild Z from X and Y, as for any two-channel normal, unless the user set it.
    let two_channel = f
        .analysis
        .dds_dxgi
        .and_then(crate::dds::from_dxgi)
        .is_some_and(|d| {
            matches!(
                d,
                crate::dds::DdsPixelFormat::Bc5 | crate::dds::DdsPixelFormat::Rg8
            )
        });
    if takes_green(st) && two_channel && !field_explicit(ctx, &prov, "channels.reconstruct_z") {
        profile.channels.reconstruct_z = true;
        prov.insert(
            "channels.reconstruct_z".into(),
            Provenance::AnalysisFact("two-channel-dds".into()),
        );
    }
    // DECISIONS Note W6: only a base normal reads `rnm`, so another sub-type keeps the default (and its hash).
    if st.rnm_base().is_none() && !profile.rnm.is_default() {
        profile.rnm = crate::config::RnmSpec::default();
        prov.retain(|k, _| !k.starts_with("rnm."));
    }
    // DECISIONS DS4: only a cubemap reads `cubemap`, so another sub-type keeps the default (and its hash).
    if st != Subtype::Cubemap && !profile.cubemap.is_default() {
        profile.cubemap = CubemapSpec::default();
        prov.retain(|k, _| !k.starts_with("cubemap."));
    }
    // An order that resolves to the default order is the default (no re-encode); any other is stored normalised.
    if profile.rnm.order.is_some() {
        let eff = profile.rnm.effective_order();
        profile.rnm.order = (eff != crate::config::LayerId::ORDER).then_some(eff);
    }
    // Last: `filter_allowed` reads the final format, content, category and alpha policy.
    let dropped = unscoped_filter_falls_back(&mut profile, &mut prov, &sources);
    Ok((profile, prov, dropped))
}

/// DECISIONS DS4: a cubemap's `cubemap.style` fills `face_size`, `format`, `blur` (and `saturation`) where no
/// explicit layer set them, and the tag is UNORM unless set explicitly (an explicit sRGB is refused by
/// `encoding_warnings`). At sub-type-table reach without a pin, so `backend_follows_format` moves the backend with
/// the style's format.
fn cube_style_layer(
    ctx: &ResolveCtx,
    doc: &mut Value,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &mut FormatSources,
) {
    let style = doc
        .pointer("/cubemap/style")
        .and_then(|v| serde_json::from_value::<CubeStyle>(v.clone()).ok());
    let photo = doc.pointer("/cubemap/layout").and_then(Value::as_str) == Some("photo");
    let mut fill = |field: &str, value: Value| {
        if !field_explicit(ctx, prov, field) {
            let patch = to_patch(field, &value);
            apply(
                doc,
                prov,
                sources,
                &patch,
                Provenance::Builtin,
                SUBTYPE_TABLE,
            );
        }
    };
    if let Some(style) = style {
        let (size, format, blur, saturation) = match style {
            CubeStyle::VanillaMetal => (32, Format::Bc1, 0.6, Some(0.8)),
            CubeStyle::Room => (128, Format::Bc1, 0.35, None),
            CubeStyle::Eye => (256, Format::Bc7, 0.35, None),
            CubeStyle::Hd => (512, Format::Bc7, 0.1, None),
        };
        fill("cubemap.face_size", json!(size));
        fill("format", json!(format));
        fill("cubemap.blur", json!(blur));
        if let Some(s) = saturation {
            fill("cubemap.saturation", json!(s));
        }
    }
    // DS16: a single photo is soft by nature, so it blurs heavily unless `cubemap.blur` is explicit.
    if photo {
        fill("cubemap.blur", json!(0.6));
    }
    fill("color.tag", json!(ColorTag::Unorm));
}

/// DECISIONS S-mips-13 (owner, 2026-10-11): a non-box `mips.filter` this profile may not use
/// ([`crate::filter::filter_allowed`]) becomes box when the last layer that set it was unscoped (an unscoped `--set`,
/// a preset or config `defaults` table); `resolve_decided` reports it as CFG-FILTER-IGNORED (Info). Set at sub-type
/// or file scope (a `subtypes.<st>` table, a sub-type- or file-scoped `--set`, a rule, `[files]`) it is kept and
/// `check_profile` reports CFG-INVALID. Returns the dropped filter and why.
fn unscoped_filter_falls_back(
    p: &mut Profile,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &FormatSources,
) -> Option<(MipFilter, &'static str)> {
    let why = crate::filter::filter_allowed(p).err()?;
    if sources.filter?.level != GENERIC.level {
        return None;
    }
    let dropped = p.mips.filter;
    p.mips.filter = MipFilter::Box;
    prov.insert("mips.filter".into(), Provenance::Fallback);
    Some((dropped, why))
}

/// Carried fix 4 (DECISIONS Note T10): an explicit `channels.flip_green` that contradicts the resolved
/// `green_convention` (OpenGL needs the flip, DirectX does not). The explicit field is kept.
fn green_conflict(
    ctx: &ResolveCtx,
    p: &Profile,
    prov: &BTreeMap<String, Provenance>,
) -> Option<Warning> {
    let convention = p.channels.green_convention?;
    if !takes_green(p.subtype) || !field_explicit(ctx, prov, "channels.flip_green") {
        return None;
    }
    let wants = convention == GreenConvention::Opengl;
    (p.channels.flip_green != wants).then(|| {
        let id = serde_json::to_value(convention).unwrap_or_default();
        Warning::new(
            Code::CfgGreenConflict,
            format!(
                "channels.flip_green = {} contradicts green_convention = {} (from {}); flip_green is kept",
                p.channels.flip_green,
                id.as_str().unwrap_or("?"),
                prov.get("channels.green_convention")
                    .map_or("builtin".to_string(), ToString::to_string)
            ),
        )
        .with_data(json!({ "flip_green": p.channels.flip_green, "convention": convention }))
    })
}

/// The CFG-FILTER-IGNORED notice (S-mips-13): the owner's line, plus the reason when the texture is a colour map.
fn filter_ignored_notice(p: &Profile, f: &FileFacts, (dropped, why): (MipFilter, &str)) -> Warning {
    let id = serde_json::to_value(dropped).unwrap_or_default();
    let id = id.as_str().unwrap_or("?");
    let file = f
        .input
        .rel
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut msg = format!("mips.filter = {id} only affects colour maps; {file} uses box");
    // A cutout, linear or HDR colour map is a colour map: say what bars the filter there.
    if matches!(p.category, Category::Color | Category::Hdr) {
        msg = format!("{msg}: {why}");
    }
    Warning::new(Code::CfgFilterIgnored, msg)
        .with_data(json!({ "filter": dropped, "used": MipFilter::Box, "reason": why }))
}

/// M6 spec §5: the shader's member defaults, written only where the field still holds a built-in value.
fn shader_layer(
    ctx: &ResolveCtx,
    shader: crate::shader::Shader,
    st: Subtype,
    doc: &mut Value,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &mut FormatSources,
) {
    for (field, value) in crate::shader::member_defaults(shader, st) {
        if !field_explicit(ctx, prov, field) {
            apply(
                doc,
                prov,
                sources,
                &to_patch(field, &value),
                Provenance::Shader(shader.id().into()),
                GENERIC,
            );
        }
    }
}

/// `skin_wet` has two forms (DECISIONS Note R-skin-1): a tangent-space normal with a binary mask in alpha (BC7, the
/// sub-type default) and a grayscale height map (BC4). Grey content (the `VAL-M-GRAYSCALE` test) resolves the
/// height form as an analysis fact, below every explicit layer: a preset, config, rule or `--set` that names the
/// `format` (or the alpha policy) keeps it.
fn skin_wet_form(
    ctx: &ResolveCtx,
    f: &FileFacts,
    doc: &mut Value,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &mut FormatSources,
) {
    if !f.analysis.grayscale {
        return;
    }
    let fact = || Provenance::AnalysisFact("skin-wet-form".into());
    if !field_explicit(ctx, prov, "format") {
        apply(
            doc,
            prov,
            sources,
            &to_patch("format", &json!("bc4")),
            fact(),
            GENERIC,
        );
    }
    if !alpha_policy_explicit(ctx, prov) {
        apply(
            doc,
            prov,
            sources,
            &to_patch("alpha.policy", &json!("remove")),
            fact(),
            GENERIC,
        );
    }
}

/// The `SKIN-WET-FORM` notice: which form a `skin_wet` file was read as, and why.
fn skin_wet_notice(p: &Profile, f: &FileFacts, prov: &BTreeMap<String, Provenance>) -> Warning {
    let height = p.category == Category::Grayscale;
    let grey = f.analysis.grayscale;
    let format_src = prov
        .get("format")
        .map_or("builtin".to_string(), |s| s.to_string());
    let (form, why) = match (height, grey) {
        (true, true) => (
            "height",
            "grey content (R = G = B): a height map".to_string(),
        ),
        (true, false) => (
            "height",
            format!(
                "colour content, but format {} (from {format_src}) asks for a red-only height map",
                p.format.id()
            ),
        ),
        (false, _) => (
            "normal_mask",
            "colour content: a tangent-space normal with a binary mask in alpha".to_string(),
        ),
    };
    Warning::new(
        Code::SkinWetForm,
        format!(
            "skin_wet is read as a {}: {why}; written as {}",
            if height {
                "height map"
            } else {
                "normal map with a mask in alpha"
            },
            p.format.id()
        ),
    )
    .with_data(json!({ "form": form, "format": p.format }))
}

/// DECISIONS O2/S3: a float source with a finite R, G or B above 1.0.
pub fn is_hdr_source(f: &FileFacts) -> bool {
    f.info.is_float && f.analysis.channels[..3].iter().any(|c| c.max > 1.0)
}

/// The name messages use for an HDR format: "BC6H" (not the id `bc6h_uf16`) or "RGBA16F".
fn hdr_name(f: Format) -> String {
    if f == Format::Bc6hUf16 {
        "BC6H".into()
    } else {
        f.id().to_uppercase()
    }
}

/// DECISIONS O2/S3, the fact `analysis:hdr-source`: a `pbr_emissive` whose BC6H format no layer chose explicitly
/// becomes BC7 unless the source is HDR; then, on every `pbr_emissive`, `color.tag` follows the final format
/// (`n/a` for BC6H, else `srgb`) unless a layer set it. Runs after every layer, so an explicit `format` or tag from
/// a user/project preset, config, rule, `[files]` or `--set` is kept. On the switch to BC7 an explicit backend is
/// kept when it can write BC7 or was pinned (`backend_follows_format`'s rule; trace-debug wave 2 #1): a preset or
/// config table's `ispc_bc6h` follows the fact to bc7e, a file-scoped `--set` still reports CFG-INVALID.
fn emissive_hdr_choice(
    ctx: &ResolveCtx,
    p: &mut Profile,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &FormatSources,
    f: &FileFacts,
) {
    if p.subtype != Subtype::PbrEmissive {
        return;
    }
    let fact = || Provenance::AnalysisFact("hdr-source".into());
    if p.format == Format::Bc6hUf16 && !field_explicit(ctx, prov, "format") && !is_hdr_source(f) {
        p.format = Format::Bc7;
        prov.insert("format".into(), fact());
        let keep = field_explicit(ctx, prov, "encoder.backend")
            && (backend_fits(p.encoder.backend, p.format)
                || sources.backend.is_some_and(|(r, _)| r.pins));
        if !keep {
            p.encoder.backend = default_backend(p.format);
            prov.insert("encoder.backend".into(), fact());
        }
    }
    if !field_explicit(ctx, prov, "color.tag") {
        let tag = if p.format.is_hdr() {
            ColorTag::NotApplicable
        } else {
            ColorTag::Srgb
        };
        if p.color.tag != tag {
            p.color.tag = tag;
            prov.insert("color.tag".into(), fact());
        }
    }
}

/// The `FMT-HDR-SOURCE` notice: why a `pbr_emissive` is or is not BC6H (DECISIONS S3).
fn hdr_source_notice(
    ctx: &ResolveCtx,
    p: &Profile,
    f: &FileFacts,
    prov: &BTreeMap<String, Provenance>,
) -> Warning {
    let hdr = is_hdr_source(f);
    let peak = f.info.is_float.then(|| {
        f.analysis.channels[..3]
            .iter()
            .map(|c| c.max)
            .fold(0.0f32, f32::max)
    });
    let source = match peak {
        Some(v) if hdr => format!("float source, peak {v:.2} > 1.0: HDR"),
        Some(v) => format!("float source, peak {v:.2} ≤ 1.0: no HDR"),
        None => format!("{}-bit source, no HDR", f.info.bit_depth),
    };
    let by_fact =
        matches!(prov.get("format"), Some(Provenance::AnalysisFact(x)) if x == "hdr-source");
    let explicit = !by_fact && field_explicit(ctx, prov, "format");
    let chosen = match (p.format, p.color.tag) {
        (f, _) if f.is_hdr() => hdr_name(f),
        (fmt, ColorTag::Srgb) => format!("{} sRGB", fmt.id().to_uppercase()),
        (fmt, _) => format!("{} UNORM", fmt.id().to_uppercase()),
    };
    let message = if explicit {
        let src = prov
            .get("format")
            .map_or("builtin".into(), |s| s.to_string());
        format!(
            "{source}; format {} from {src} (the HDR check is skipped)",
            p.format.id()
        )
    } else {
        format!("{source} → {chosen}")
    };
    Warning::new(Code::FmtHdrSource, message).with_data(json!({
        "hdr": hdr, "float": f.info.is_float, "peak": peak, "format": p.format, "explicit": explicit
    }))
}

/// Spec §4.1 (`pbr_emissive`, `hdr_sky`): a float source that goes to BC6H is linear radiance, so
/// `color.content` resolves `linear` as an analysis fact (`analysis:float-source`): above presets, config
/// and rules, below suffix facts and `--set`. It reads the resolved format, so it runs after the layers.
/// A float source written to an LDR format keeps the sub-type's content, which then names only the output encoding:
/// the source is linear light either way (DECISIONS S1).
fn float_source_is_linear(p: &mut Profile, prov: &mut BTreeMap<String, Provenance>, f: &FileFacts) {
    const FIELD: &str = "color.content";
    let overruled = prov
        .get(FIELD)
        .is_some_and(|s| *s >= Provenance::Suffix(String::new()));
    if f.info.is_float && p.format.is_hdr() && !overruled {
        p.color.content = ColorContent::Linear;
        prov.insert(
            FIELD.into(),
            Provenance::AnalysisFact("float-source".into()),
        );
    }
}

/// Whether `b` can write format `f`.
pub fn backend_fits(b: Backend, f: Format) -> bool {
    match b {
        Backend::Bc7e => f == Format::Bc7,
        Backend::Rgbcx => matches!(f, Format::Bc1 | Format::Bc3 | Format::Bc4 | Format::Bc5),
        Backend::IspcBc6h => f == Format::Bc6hUf16,
        Backend::Nvtt => matches!(f, Format::Bc7 | Format::Bc6hUf16),
        Backend::Uncompressed => !f.is_block(),
    }
}

/// The built-in encoder for a format (spec §3.2 `encoder.backend` default "Format"); never NVTT.
pub fn default_backend(f: Format) -> Backend {
    match f {
        Format::Bc1 | Format::Bc3 | Format::Bc4 | Format::Bc5 => Backend::Rgbcx,
        Format::Bc6hUf16 => Backend::IspcBc6h,
        Format::Bc7 => Backend::Bc7e,
        Format::Rgba8 | Format::R8 | Format::Rg8 | Format::Rgba16f => Backend::Uncompressed,
    }
}

/// A layer that changes only `format` takes the backend with it. A backend that does not fit the format is
/// kept only when the patch that set the format also set it, or when it was pinned (sub-type- or file-scoped
/// `--set`, or a rule) at least as specifically as the format; `check_profile` then reports CFG-INVALID.
/// Otherwise it becomes the format's default backend, with the format's provenance. So a config or preset
/// `defaults.encoder.backend = "nvtt"` applies where NVTT fits and leaves BC4/BC5 sub-types on rgbcx.
fn backend_follows_format(
    p: &mut Profile,
    prov: &mut BTreeMap<String, Provenance>,
    sources: &FormatSources,
) {
    if backend_fits(p.encoder.backend, p.format) {
        return;
    }
    let keep = match (sources.format, sources.backend) {
        (Some((fr, fp)), Some((br, bp))) => bp == fp || (br.pins && br.level >= fr.level),
        _ => false,
    };
    if keep {
        return;
    }
    p.encoder.backend = default_backend(p.format);
    if let Some(fmt) = prov.get("format").cloned() {
        prov.insert("encoder.backend".into(), fmt);
    }
}

/// Share of pixels whose alpha is ≤ 5 or ≥ 250.
pub fn near_binary(a: &Analysis) -> f64 {
    let n = (a.width as f64 * a.height as f64).max(1.0);
    (a.alpha_hist.le5 + a.alpha_hist.ge250) as f64 / n
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable { fallback: Option<Subtype> },
}

/// The "—" cells of spec §4.2, with the fallback offered to the user.
pub fn availability(t: Target, s: Subtype) -> Availability {
    use Subtype::*;
    let un = |f: Option<Subtype>| Availability::Unavailable { fallback: f };
    match (t, s) {
        (Target::Cs, _) => Availability::Available,
        (_, PbrAlbedo) => un(Some(Diffuse)),
        (_, PbrEmissive) => un(Some(Glow)),
        (_, PbrNormal) => un(Some(Normal)),
        (_, PbrSubsurface | PbrFuzz | CoatNormal | Rmaos | SkinRfaos | Flow | HdrSky) => un(None),
        (_, SkinAlbedo | SkinSk | SkinWet) => un(None),
        _ => Availability::Available,
    }
}

/// Whether the sub-type pickers offer `s` under `t`: available, and never `complex_grass`, which the set shader or the
/// GRAS auto-detect chooses (DECISIONS X3). A file tagged `complex_grass` still encodes (`availability` is unchanged).
pub fn offered(t: Target, s: Subtype) -> bool {
    availability(t, s) == Availability::Available && s != Subtype::ComplexGrass
}

/// What an unresolved job is waiting for (DECISIONS N9), so a GUI can offer the right control.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// The sub-type is a suggestion nobody confirmed.
    Subtype,
    /// The alpha meaning (policy) depends on the mesh and nobody chose it.
    AlphaMeaning,
    /// A single-image cube has no clear layout and nobody chose one (cubemap GUI, DECISIONS DT1).
    CubeLayout,
}

/// What happens to a file (spec §5.2, §10). Serialises as `{"state": "ready"}`,
/// `{"state": "blocked", "codes": [...]}` and so on.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Ready,
    /// A sub-type or alpha meaning still needs a decision (rule, flag or accepted high-confidence suggestion).
    Unresolved {
        reasons: Vec<String>,
        /// What each reason is waiting for, so a GUI can offer the right control.
        #[serde(default)]
        needs: Vec<Need>,
    },
    /// A raw single-channel map (spec §4.4): a channel-packing input (§6.4), not converted on its own and not an
    /// error; a raw map waiting for `pack::plan::plan_packing`, which consumes it into a recipe or re-tags it
    /// `PACK-UNUSED-INPUT`.
    PackingInput {
        role: RawMap,
    },
    /// An RNM normal layer or mask (M6 Track F2 spec §2.2): never converted on its own; `rnm::plan::plan_rnm`
    /// attaches it to its base normal's job or tags it `RNM-*`.
    NormalLayer {
        term: crate::rnm::LayerTermId,
    },
    /// Blocking codes not acknowledged with `--allow`.
    Blocked {
        codes: Vec<Code>,
    },
    Error {
        codes: Vec<Code>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ResolvedJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub game_path: String,
    pub set_key: String,
    pub profile: Profile,
    /// Dotted field path → where its value came from; includes `subtype`, `category` and `target`.
    pub provenance: BTreeMap<String, Provenance>,
    pub suggestions: Vec<Suggestion>,
    pub warnings: Vec<Warning>,
    pub status: JobStatus,
    /// The suffix alias this file matched (spec §3.7); its preset chain resolved the profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix_alias: Option<AliasUse>,
    /// The source's size and modification time when `Engine::plan` read it (final review I7). `run` converts the
    /// file only while they still match; `None` (a job not made by `plan`) is not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceStamp>,
    /// How a packed output is built from its raw sources (spec §6.4); `None` for an ordinary file. For a packed
    /// job `input` is `pack.sources[0].path` (DECISIONS Note M8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<crate::pack::PackJob>,
    /// A roughness member's normal-variance bake (spec §6.2, DECISIONS R4): the `_n` it reads and how. `None` for
    /// every other job and for a member that does not bake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bake: Option<crate::bake::BakeJob>,
    /// M6 (DECISIONS Note S): the shader picked for this job's set; `None` for a job not made by `plan`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader: Option<crate::shader::ShaderPick>,
    /// M6 Track F2: a base normal's RNM layers (`rnm::plan::plan_rnm`); `None` for every other job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rnm: Option<crate::rnm::RnmJob>,
    /// Track A (DECISIONS Note CN11): how a derived output is built from a PBR set's inputs; `None` for every other
    /// job. For a derived job `input` is `derive.sources[0].path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derive: Option<crate::derive::DeriveJob>,
    /// Cubemap v1 (DECISIONS DS8): how a cube is built from its source file or six face files
    /// (`cube::plan::plan_cubes`); `None` for every other job. For a cube job `input` is `cube.sources[0].path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cube: Option<crate::cube::CubeJob>,
}

impl ResolvedJob {
    /// The RNM layers the encode blends: `rnm` while `profile.rnm.enabled` and it lists a layer.
    pub fn active_rnm(&self) -> Option<&crate::rnm::RnmJob> {
        self.rnm
            .as_ref()
            .filter(|r| self.profile.rnm.enabled && !r.layers.is_empty())
    }
}

/// A source file's size and modification time, to tell whether it changed between planning and conversion.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct SourceStamp {
    pub size: u64,
    /// Nanoseconds since the Unix epoch.
    pub mtime_ns: u64,
}

impl SourceStamp {
    /// The stamp of `path` now; `None` when it cannot be read (the file is gone or has no usable timestamp).
    pub fn of(path: &std::path::Path) -> Option<SourceStamp> {
        let m = std::fs::metadata(path).ok()?;
        let since = m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(SourceStamp {
            size: m.len(),
            mtime_ns: u64::try_from(since.as_nanos()).unwrap_or(u64::MAX),
        })
    }
}

/// Formats that store alpha in v1. BC1 is opaque only (spec §3.3, DECISIONS Note F), although
/// `Format::has_alpha` counts its 1-bit mode.
fn stores_alpha(f: Format) -> bool {
    matches!(
        f,
        Format::Bc3 | Format::Bc7 | Format::Rgba8 | Format::Rgba16f
    )
}

/// `normal_len_err` above this is "far from unit length" (CHAN-NORMAL-LENGTH).
const NORMAL_LEN_TOL: f64 = 0.1;

/// Mean decoded normal length below which mip 0 is "noticeably short" (CHAN-NORMAL-SHORT, DECISIONS R1).
const NORMAL_SHORT_MEAN: f64 = 0.9;
const NORMAL_SHORT_RENORMALISED: &str =
    "mip 0 is renormalised to unit length (set mips.normal_base = keep to store it as authored)";
const NORMAL_SHORT_KEPT: &str = "mip 0 is stored as authored (mips.normal_base = keep)";

/// Spec §4.3 rule 5: content that contradicts the decided sub-type. Reported, never blocking.
fn contradictions(p: &Profile, f: &FileFacts) -> Vec<String> {
    let a = f.analysis;
    let st = p.subtype;
    let mut v = vec![];
    // Not-unit normals are CHAN-NORMAL-LENGTH's alone (Note DP4).
    if p.category == Category::Normal && a.grayscale {
        v.push(format!("{} source is grayscale (R = G = B)", st.id()));
    }
    // DECISIONS AH1/AH2: only a written solid 255 reads as a plain envmask; a 0 fill keeps complex material on.
    if st == Subtype::ComplexMaterial
        && a.alpha == AlphaContent::Constant255
        && !crate::ops::complex_alpha_zero(p, f.info.has_alpha)
    {
        v.push(
            "complex_material with parallax on and solid 255 alpha: this reads as a plain envmask"
                .into(),
        );
    }
    // A tangent-space normal decodes to unit length with Z >= 0, so blue sits high.
    if p.category == Category::Color
        && a.normal_len_err < NORMAL_LEN_TOL
        && a.channels[2].mean >= 0.75
    {
        v.push(format!(
            "{} source looks like a tangent-space normal map (unit length, blue mostly high)",
            st.id()
        ));
    }
    v
}

/// BC6H and RGBA16F are written only under cs [D#3], DECISIONS S4. An error `--allow` cannot pass.
fn hdr_outside_cs(f: Format, t: Target) -> bool {
    f.is_hdr() && t != Target::Cs
}

/// Complex grass is BC7 UNORM (design §3.5, owner 2026-10-13: "BC7 linear only").
fn grass_encoding_bad(f: Format, tag: ColorTag) -> bool {
    f != Format::Bc7 || tag == ColorTag::Srgb
}

/// Whether `st` may take an HDR format (owner, 2026-10-13): never a normal or a data map (linear UNORM only), and
/// of the PBR colour maps only the emissive (PBR `_f` and `_g` are sRGB only, BC6H the option for an emissive `_g`).
fn takes_hdr(st: Subtype) -> bool {
    match st.category() {
        Category::Normal | Category::Grayscale | Category::MixedPacked => false,
        Category::Color => !st.is_pbr_color() || st == Subtype::PbrEmissive,
        Category::Hdr => true,
    }
}

/// The checks of `check_profile` that read only the sub-type, its category, the target, the format and the tag: the
/// one source of [`allowed_encodings`], so a format picker and validation never disagree.
fn encoding_warnings(
    st: Subtype,
    category: Category,
    target: Target,
    format: Format,
    tag: ColorTag,
) -> Vec<Warning> {
    use Code::*;
    let mut w = vec![];
    let mut add = |c: Code, m: String| w.push(Warning::new(c, m));
    // An HDR format (BC6H, RGBA16F) has no _SRGB or UNORM form, and every other format needs one (spec §3.2, §4.2).
    let hdr = format.is_hdr();
    if hdr != (tag == ColorTag::NotApplicable) {
        let fmt = format.id().to_uppercase();
        let msg = if hdr {
            format!(
                "{} has no _SRGB or UNORM form; set color.tag=n/a",
                hdr_name(format)
            )
        } else if st.is_pbr_color() && target == Target::Cs {
            format!(
                "color.tag n/a is only for BC6H or RGBA16F on {}: set color.tag=srgb for a non-BC6H _g ({fmt})",
                st.id()
            )
        } else {
            format!(
                "color.tag n/a is only for BC6H or RGBA16F: set color.tag=unorm for a non-BC6H {} ({fmt})",
                st.id()
            )
        };
        add(CfgInvalid, msg);
    }
    // BC4, BC5, R8 and RG8 have no _SRGB DXGI format: the file would be written UNORM (owner, 2026-10-13, for BC4/BC5).
    if matches!(format, Format::Bc4 | Format::Bc5 | Format::R8 | Format::Rg8)
        && tag == ColorTag::Srgb
    {
        add(
            CfgInvalid,
            format!(
                "{} has no _SRGB form; set color.tag=unorm",
                format.id().to_uppercase()
            ),
        );
    }
    // Owner, 2026-10-11: a packed map and an HDR format are mutually exclusive (packed channels are 8-bit data,
    // often with alpha). Owner, 2026-10-13: so are a normal, a grayscale map and non-emissive PBR colour.
    if hdr && category == Category::MixedPacked {
        add(
            CfgInvalid,
            format!(
                "{} is a packed map; {} cannot store it, use bc7",
                st.id(),
                hdr_name(format)
            ),
        );
    } else if hdr && !takes_hdr(st) && target == Target::Cs {
        // Outside cs the HDR error at the end of `check_profile` is the one root cause.
        add(
            CfgInvalid,
            format!(
                "{} is not for {}: a normal or data map is linear UNORM, PBR colour but emissive is sRGB; choose another format",
                hdr_name(format),
                st.id()
            ),
        );
    }
    let srgb = tag == ColorTag::Srgb;
    if srgb && target != Target::Cs {
        add(
            TagSrgbVanilla,
            "_SRGB is not usable under vanilla shaders [D#4]".into(),
        );
    } else if srgb && !st.is_pbr_color() {
        add(
            TagSrgbNonpbr,
            "_SRGB on a non-PBR sub-type renders darker [SC §6]".into(),
        );
    }
    // Blocking since the owner's 2026-10-13 ruling: PBR colour is sRGB only.
    if tag == ColorTag::Unorm && st.is_pbr_color() && !hdr && target == Target::Cs {
        add(
            TagUnormPbr,
            "UNORM on PBR colour looks washed out with linear lighting off".into(),
        );
    }
    if category == Category::Normal
        && matches!(format, Format::Bc4 | Format::Bc5 | Format::R8 | Format::Rg8)
    {
        add(
            FmtBc5Normal,
            "BC4/BC5/R8/RG8 on a normal: Skyrim reads Z directly, so Z decodes -1 and alpha is lost [SC §2]".into(),
        );
    }
    if st == Subtype::LandscapeDiffuse && format == Format::Bc1 {
        add(
            FmtLandscapeBc1,
            "BC1 destroys the landscape snow mask / height alpha".into(),
        );
    }
    if st == Subtype::ComplexGrass && grass_encoding_bad(format, tag) {
        add(
            CfgInvalid,
            "complex_grass is written as BC7 UNORM at its own size (resize would have to run on each half)"
                .into(),
        );
    }
    if st == Subtype::Cubemap
        && !(matches!(
            format,
            Format::Bc1 | Format::Bc3 | Format::Bc7 | Format::Rgba8
        ) && tag == ColorTag::Unorm)
    {
        add(
            CfgInvalid,
            "cubemaps are written as BC1, BC3, BC7 or RGBA8, UNORM".into(),
        );
    }
    w
}

/// How a cubemap source is laid out: `cubemap.layout`, else six face files for a face-tagged name, else a cube DDS
/// (a 6:1 strip), else the aspect ratio; `None` when nothing says (DECISIONS DS7).
fn cube_layout(p: &Profile, f: &FileFacts) -> Option<crate::config::CubeLayout> {
    let a = f.analysis;
    cube_layout_of(p, &f.input.path, a.dds_cube, (a.width, a.height))
}

/// `cube_layout` from the loaded size and the cube-DDS flag; `cube::plan::plan_cubes` reads the same order.
pub(crate) fn cube_layout_of(
    p: &Profile,
    path: &std::path::Path,
    dds_cube: bool,
    (w, h): (u32, u32),
) -> Option<crate::config::CubeLayout> {
    use crate::config::CubeLayout;
    let stem = path.file_stem().and_then(|s| s.to_str());
    // A cube DDS has loaded as its six faces side by side, whatever its name or `cubemap.layout` say.
    dds_cube
        .then_some(CubeLayout::Hstrip)
        .or(p.cubemap.layout)
        .or_else(|| {
            stem.and_then(crate::cube::plan::face_tag)
                .map(|_| CubeLayout::Faces)
        })
        .or_else(|| crate::cube::layout::detect(w, h))
}

/// A cubemap's mips and `cubemap.*` ranges (DECISIONS DS4).
fn cube_checks(p: &Profile, add: &mut impl FnMut(Code, String)) {
    if matches!(
        p.mips.levels,
        MipLevels::Keyword(MipKeyword::None) | MipLevels::Count(0..=1)
    ) {
        add(
            Code::CubeNoMips,
            "a cubemap without mips sparkles on curved surfaces, and CS reads its last mip as the average colour"
                .into(),
        );
    }
    let c = &p.cubemap;
    let mut bad = |what: String| add(Code::CfgInvalid, what);
    if let Some(n) = c
        .face_size
        .filter(|n| !(n.is_power_of_two() && (4..=4096).contains(n)))
    {
        bad(format!(
            "cubemap.face_size {n} is not a power of two in 4..=4096"
        ));
    }
    if let Some(r) = c
        .face_rotate
        .iter()
        .find(|r| ![0, 90, 180, 270].contains(*r))
    {
        bad(format!("cubemap.face_rotate {r} is not 0, 90, 180 or 270"));
    }
    let ranges = [
        ("yaw", Some(c.yaw), -360.0..=360.0),
        ("blur", c.blur, 0.0..=1.0),
        ("exposure", Some(c.exposure), -10.0..=10.0),
        ("brightness", Some(c.brightness), -1.0..=1.0),
        ("contrast", Some(c.contrast), -1.0..=1.0),
        ("midtones", Some(c.midtones), 0.1..=10.0),
        ("saturation", Some(c.saturation), 0.0..=2.0),
        ("highlight_clamp", c.highlight_clamp, 1.0..=f64::MAX),
        ("horizon", Some(c.horizon), -45.0..=45.0),
    ];
    for (key, v, range) in ranges {
        if let Some(v) = v.filter(|v| !range.contains(v)) {
            bad(format!("cubemap.{key} {v} outside {range:?}"));
        }
    }
}

/// The category a profile of `st` written as `format` has: a `skin_wet` height form (grey, or BC4) is a grayscale
/// map (DECISIONS Note R-skin-1).
fn category_of(st: Subtype, format: Format, grayscale: bool) -> Category {
    if st == Subtype::SkinWet && (grayscale || format == Format::Bc4) {
        Category::Grayscale
    } else {
        st.category()
    }
}

/// Every (format, tag) pair a format picker may offer for `st` under `target` (owner, 2026-10-13): the pairs
/// `check_profile` raises nothing blocking or worse for from the format and tag alone. Empty for a sub-type
/// unavailable under `target` (CFG-UNAVAILABLE: nothing is written). A grey `skin_wet` is judged as the greyscale
/// map it is only by [`allowed_encodings_for`].
pub fn allowed_encodings(st: Subtype, target: Target) -> Vec<(Format, ColorTag)> {
    allowed_encodings_for(st, target, false)
}

/// [`allowed_encodings`] for a file whose analysis is (`grayscale`) or is not greyscale, as `check_profile` judges
/// a grey `skin_wet` (trace-debug #31, Note BQ).
pub fn allowed_encodings_for(
    st: Subtype,
    target: Target,
    grayscale: bool,
) -> Vec<(Format, ColorTag)> {
    if availability(target, st) != Availability::Available {
        return vec![];
    }
    let formats = [
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
        Format::Bc6hUf16,
        Format::Bc7,
        Format::Rgba8,
        Format::R8,
        Format::Rg8,
        Format::Rgba16f,
    ];
    let tags = [ColorTag::Srgb, ColorTag::Unorm, ColorTag::NotApplicable];
    formats
        .into_iter()
        .flat_map(|f| tags.map(|t| (f, t)))
        .filter(|&(f, t)| {
            !hdr_outside_cs(f, target)
                && encoding_warnings(st, category_of(st, f, grayscale), target, f, t)
                    .iter()
                    .all(|w| w.severity < Severity::Blocking)
        })
        .collect()
}

/// The §10 checks that Resolve can make from the profile, the file's path and its analysis.
///
/// A sub-type unavailable under the target (spec §3.1, §4.2) reports CFG-UNAVAILABLE only: that profile is never
/// written, so its format, tag, alpha and dimension checks would be a cascade of the one root cause.
pub fn check_profile(
    p: &Profile,
    f: &FileFacts,
    alpha_explicit: bool,
    nvtt: Option<&crate::nvtt::NvttCell>,
) -> Vec<Warning> {
    use Code::*;
    let a = f.analysis;
    let st = p.subtype;
    if let Availability::Unavailable { fallback } = availability(p.target, st) {
        let fix = match fallback {
            Some(fb) => format!(
                "accept the fallback {} with --allow CFG-UNAVAILABLE",
                fb.id()
            ),
            None => "no fallback; change the sub-type or the target".to_string(),
        };
        return vec![Warning::new(
            CfgUnavailable,
            format!("{} is unavailable under {:?}; {fix}", st.id(), p.target),
        )];
    }
    let mut w = vec![];
    let mut add = |c: Code, m: String| w.push(Warning::new(c, m));
    // An HDR format (BC6H, RGBA16F) outside cs cannot be acknowledged: an error, added after the other checks.
    let hdr_outside_cs = hdr_outside_cs(p.format, p.target);
    if a.dds_cube && st != Subtype::Cubemap {
        add(
            CfgInvalid,
            "a cube DDS can only be re-encoded as a cubemap".into(),
        );
    }
    if a.dds_cube
        && st == Subtype::Cubemap
        && p.cubemap
            .layout
            .is_some_and(|l| l != crate::config::CubeLayout::Hstrip)
    {
        add(
            CfgInvalid,
            "a cube DDS loads as its six faces; remove cubemap.layout for this file".into(),
        );
    }
    if !ALPHA_THRESHOLD.contains(&p.alpha.threshold) {
        add(
            CfgInvalid,
            format!(
                "alpha.threshold {} outside {ALPHA_THRESHOLD:?}",
                p.alpha.threshold
            ),
        );
    }
    // M2.5 (spec §6.4): the neutral roughness (0..=1) of the sub-types whose recipe reads it: `_rmaos`/`_rfaos` and
    // complex `_m`. The check also fires for a pre-packed file of that sub-type, which never reads the value; that is
    // harmless (the value is a profile-wide setting, so a bad one is worth flagging wherever it shows up).
    if (st.is_roughness_member() || st == Subtype::ComplexMaterial)
        && !(0.0..=1.0).contains(&p.packing.roughness_default)
    {
        add(
            CfgInvalid,
            format!(
                "packing.roughness_default {} outside 0..=1",
                p.packing.roughness_default
            ),
        );
    }
    // Spec §6.2 clamp t (DECISIONS Note E, R2): read where a bake writes, a roughness member or a `_cnr`.
    if (st.is_roughness_member() || st == Subtype::CoatNormal)
        && (p.mips.bake_clamp.is_nan() || p.mips.bake_clamp <= 0.0 || p.mips.bake_clamp > 1.0)
    {
        add(
            CfgInvalid,
            format!("mips.bake_clamp {} outside (0, 1]", p.mips.bake_clamp),
        );
    }
    // M6 Track F2 (DECISIONS Note W7): layer strengths and the AO power, on a base normal.
    if st.rnm_base().is_some() {
        for id in crate::config::LayerId::ORDER {
            let s = p.rnm.layer(id).strength;
            if !(0.0..=2.0).contains(&s) {
                add(
                    CfgInvalid,
                    format!("rnm.{}.strength {s} outside 0..=2", id.id()),
                );
            }
        }
        let pw = p.rnm.ao_power();
        if !(0.1..=8.0).contains(&pw) {
            add(
                CfgInvalid,
                format!("rnm.bent.ao_power {pw} outside 0.1..=8"),
            );
        }
    }
    // Only where rgbcx is the resolved backend (BC1/BC3/BC4/BC5): an out-of-range level would trip rgbcx's
    // assert inside an encode task, and no other backend reads the field.
    if p.encoder.backend == Backend::Rgbcx && p.encoder.rgbcx_level > encoders_sys::RGBCX_MAX_LEVEL
    {
        add(
            CfgInvalid,
            format!(
                "encoder.rgbcx_level {} outside 0..={}",
                p.encoder.rgbcx_level,
                encoders_sys::RGBCX_MAX_LEVEL
            ),
        );
    }
    if !backend_fits(p.encoder.backend, p.format) {
        add(
            CfgInvalid,
            format!(
                "encoder {:?} cannot write {:?}",
                p.encoder.backend, p.format
            ),
        );
    }
    // The `max` level is bc7e patch 0002 (docs/research/bc7e-tuning.md): without the patch-0002 objects the level
    // does not exist in the encoder, so refuse it here rather than inside an encode task.
    if p.encoder.backend == Backend::Bc7e
        && p.encoder.effort == Effort::Max
        && !encoders_sys::BC7E_MAX_NATIVE
    {
        add(
            CfgInvalid,
            "encoder.effort max needs the bc7e patch-0002 objects (tools/regen-ispc.ps1); use slowest".to_string(),
        );
    }
    // An explicit `legacy` header needs a legacy FourCC: BC1/BC3/BC4/BC5 UNORM only. `auto` falls back to DX10.
    if p.header == HeaderForm::Legacy
        && resolve_header(p.format, p.srgb_tag(), HeaderForm::Legacy) != ResolvedHeader::Legacy
    {
        let what = if p.srgb_tag() { "_SRGB " } else { "" };
        add(
            CfgInvalid,
            format!(
                "no legacy FourCC for {what}{:?}; use dx10 (header=dx10 or auto)",
                p.format
            ),
        );
    }
    if let Some(s) = &p.channels.swizzle {
        if s.len() != 4 || !s.chars().all(|c| "rgba01".contains(c)) {
            add(
                CfgInvalid,
                format!("channels.swizzle '{s}' must be 4 of r,g,b,a,0,1"),
            );
        }
    }
    let hdr = p.format.is_hdr();
    let encoding = encoding_warnings(st, p.category, p.target, p.format, p.color.tag);
    w.extend(encoding);
    let mut add = |c: Code, m: String| w.push(Warning::new(c, m));
    if let Err(why) = crate::filter::filter_allowed(p) {
        let id = serde_json::to_value(p.mips.filter).unwrap_or_default();
        add(
            CfgInvalid,
            format!("mips.filter = {}: {why}", id.as_str().unwrap_or("?")),
        );
    }
    let srgb = p.srgb_tag();
    if srgb && f.input.game_path.split('/').any(|c| c == "lod") {
        add(TagSrgbLod, "LOD shaders reject _SRGB".into());
    }
    if (p.color.content == ColorContent::Linear && srgb)
        || (p.category == Category::Color
            && p.color.content == ColorContent::Linear
            && !f.info.is_float)
    {
        add(
            TagContentMismatch,
            "colour content and DDS tag disagree".into(),
        );
    }
    // DECISIONS R10: roughness is linear data. sRGB content would decode it before the mips and the variance bake.
    if st.is_roughness_member() && p.color.content == ColorContent::Srgb {
        add(
            TagContentMismatch,
            "roughness member with sRGB content: roughness is linear data, so it is not variance-baked".into(),
        );
    }
    if p.format == Format::Bc1 && p.alpha.policy != AlphaPolicy::Remove {
        add(
            FmtBc1Alpha,
            format!(
                "BC1 in v1 is opaque only; alpha.policy {:?} needs BC7/BC3 or remove",
                p.alpha.policy
            ),
        );
    }
    // DECISIONS Note F: BC1 is opaque-only in v1, so rgbcx must never emit transparent-black texels.
    if p.format == Format::Bc1 && p.encoder.bc1_transparent_black {
        add(
            CfgInvalid,
            "encoder.bc1_transparent_black=true: BC1 is opaque-only in v1 (no 1-bit alpha), so the 3-colour mode never emits transparent-black texels; set it to false"
                .into(),
        );
    }
    if p.encoder.perceptual
        && !(p.category == Category::Color && p.alpha.policy != AlphaPolicy::Data)
    {
        add(
            FmtPerceptualData,
            "perceptual BC7 is allowed only for colour without data alpha [B2 §1]".into(),
        );
    }
    // DECISIONS Note CP: the weights reach bc7e only, and only without perceptual (bc7e's own weights win there).
    // `linear` is the plain encode everywhere, so it is never "ignored".
    if let Some(w) = p.encoder.bc7_weights.filter(|w| *w != Bc7Weights::Linear) {
        let why = if p.format != Format::Bc7 {
            Some("the format is not BC7")
        } else if p.encoder.backend != Backend::Bc7e {
            Some("only the built-in BC7 encoder (bc7e) reads them")
        } else if p.encoder.perceptual {
            Some("encoder.perceptual is on, and bc7e then uses its own weights")
        } else {
            None
        };
        if let Some(why) = why {
            let w = serde_json::to_value(w).unwrap_or_default();
            add(
                EncWeightsIgnored,
                format!("encoder.bc7_weights = {w} has no effect: {why}"),
            );
        }
    }
    // Owner 2026-10-11 (DECISIONS P25): an alpha the user explicitly removed is their decision, not a loss; a
    // PBR normal's alpha is vanilla specular that PBR does not use, so dropping it is an info. Any other
    // discarded varying alpha (an inferred remove, or a format that cannot store it) still blocks.
    let explicit_remove = alpha_explicit && p.alpha.policy == AlphaPolicy::Remove;
    if a.alpha == AlphaContent::Varying
        && !explicit_remove
        && (p.alpha.policy == AlphaPolicy::Remove || !stores_alpha(p.format))
    {
        if st == Subtype::PbrNormal {
            add(
                AlphaPbrNormalDropped,
                "vanilla specular in alpha dropped (PBR normals don't use alpha)".into(),
            );
        } else {
            add(
                LossAlphaData,
                "non-constant alpha would be discarded".into(),
            );
        }
    }
    // DECISIONS Note L: the analysis already classed near-opaque alpha as constant, so no LOSS-ALPHA-DATA above;
    // this only reports that the (noise) alpha is dropped. Under `data` the alpha is kept, so nothing is claimed.
    if a.alpha_near_opaque && (p.alpha.policy == AlphaPolicy::Remove || !stores_alpha(p.format)) {
        let min = crate::analyse::code8(a.channels[3].min);
        add(
            AlphaNearOpaque,
            format!(
                "source alpha 251–254 present (min {min}): treated as opaque (BC encode artefact); alpha removed"
            ),
        );
    }
    // DECISIONS S1: a float source is linear light; Quantise clamps it to [0, 1] and encodes it per color.content.
    // Values outside that range are lost: a warning, never a block. (IN-FLOAT-LDR is retired, DECISIONS S2.)
    if st == Subtype::Cubemap {
        // A cube's own check (DECISIONS DS7): `cubemap.highlight_clamp` is the way to keep or tame highlights.
        if f.info.is_float && a.peak_luminance > 1.0 && p.cubemap.highlight_clamp.is_none() {
            add(
                LossClampHdr,
                "highlights above 1.0 will clip: set cubemap.highlight_clamp (for example 4)"
                    .into(),
            );
        }
    } else if f.info.is_float && !hdr {
        let channels = if f.info.has_alpha { 4 } else { 3 };
        let (lo, hi) = a.channels[..channels]
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), c| {
                (lo.min(c.min), hi.max(c.max))
            });
        if lo < 0.0 || hi > 1.0 {
            add(
                LossClampHdr,
                format!("float values range {lo} to {hi}: values outside [0, 1] are clamped"),
            );
        }
    }
    if (f.info.bit_depth > 8 || f.info.is_float) && !hdr {
        add(
            LossBitdepth,
            format!(
                "{}-bit source written with 8-bit endpoints",
                f.info.bit_depth
            ),
        );
    }
    if p.category == Category::Grayscale && !a.grayscale {
        add(
            ChanNotGray,
            format!(
                "R/G/B differ by more than 4/255; keeping {:?}",
                p.channels.grayscale_source
            ),
        );
    }
    if p.alpha.policy == AlphaPolicy::Data
        && a.alpha == AlphaContent::Constant255
        && !crate::ops::complex_alpha_zero(p, f.info.has_alpha)
    {
        // DECISIONS Note L: near-opaque alpha (minimum 251..=254) is classed constant too; say what it holds.
        let msg = if a.alpha_near_opaque {
            format!(
                "data alpha is near-opaque (min {}, 251–254 is BC encode noise); remove would save bits",
                crate::analyse::code8(a.channels[3].min)
            )
        } else {
            "data alpha is constant 255; remove would save bits".to_string()
        };
        add(ChanConstantAlphaData, msg);
    }
    if p.category == Category::Normal
        && st != Subtype::Msn
        && a.normal_len_err > NORMAL_LEN_TOL
        && !p.channels.reconstruct_z
    {
        add(
            ChanNormalLength,
            format!(
                "mean |length-1| = {:.3}; is this a normal map, or does it need Z rebuilt?",
                a.normal_len_err
            ),
        );
    }
    // Owner 2026-10-11 (DECISIONS R1): a noticeably short normal map (`_msn` included), and what happens to its
    // mip 0. A rebuilt Z is unit length by construction. Reported under both `normal_base` values.
    if p.category == Category::Normal
        && !p.channels.reconstruct_z
        && a.normal_len_mean < NORMAL_SHORT_MEAN
    {
        let what = match p.mips.normal_base {
            NormalBase::Renormalize => NORMAL_SHORT_RENORMALISED,
            NormalBase::Keep => NORMAL_SHORT_KEPT,
        };
        add(
            ChanNormalShort,
            format!(
                "mean normal length {:.3} at mip 0: {what}",
                a.normal_len_mean
            ),
        );
    }
    if st.is_roughness_member() && a.blue_mostly_black {
        add(
            ChanAoInverted,
            "blue (AO) is mostly black: AO may be inverted".into(),
        );
    }
    if st.is_roughness_member() && a.red_mostly_high {
        add(
            ChanGlossAsRough,
            "red (roughness) is mostly high: is this gloss?".into(),
        );
    }
    if matches!(st, Subtype::Envmask | Subtype::ComplexMaterial)
        && p.format == Format::Bc7
        && !p.alpha.exact_opaque
        // An alpha-0 complex `_m` (DECISIONS AH1/AH2) has no opaque block to round to 254.
        && !crate::ops::complex_alpha_zero(p, f.info.has_alpha)
    {
        // BC7 only: its alpha endpoints are quantised, so opaque alpha can decode as 254 unless exact-opaque
        // handling is on. BC3's alpha block stores 255 exactly, so it carries no such risk. Not conditional on
        // the alpha policy: `remove` drops the source alpha, but BC7 still encodes an alpha channel
        // (DECISIONS Notes A and D, spec §5.8).
        add(
            AlphaExactOffM,
            "exact opaque alpha is off on an _m written as BC7; 254 may reach PGPatcher/ENB".into(),
        );
    }
    if st == Subtype::ComplexMaterial
        && p.alpha.parallax == Some(true)
        && p.alpha.policy != AlphaPolicy::Remove
        && !f.info.has_alpha
    {
        add(
            AlphaParallaxNoAlpha,
            "parallax is on but the source has no alpha channel: alpha 0 is written (complex material on, \
             parallax off)"
                .into(),
        );
    }
    // S-mips-7: the DIM checks read the size Preprocess writes. DIM-RESIZED is pushed after the last `add` (ruling F3).
    let (rw, rh) = crate::resize::target_dims((a.width, a.height), p.resize);
    // Note CL23: `drop_top_mip` writes mip 1 as the base, so the DIM checks read that size (the mips' halving rule).
    let dropped = p.drop_top_mip && crate::mips::level_count(rw, rh, &p.mips.levels) > 1;
    let (ow, oh) = if dropped {
        ((rw / 2).max(1), (rh / 2).max(1))
    } else {
        (rw, rh)
    };
    let mode = serde_json::to_value(p.resize).unwrap_or_default();
    let mut resized = ((rw, rh) != (a.width, a.height)).then(|| {
        Warning::new(
            DimResized,
            format!(
                "{}x{} resized to {ow}x{oh} (resize = {}{})",
                a.width,
                a.height,
                mode.as_str().unwrap_or("?"),
                if dropped { ", drop_top_mip" } else { "" }
            ),
        )
        .with_data(json!({ "from": [a.width, a.height], "to": [ow, oh] }))
    });
    // A cube's source size is not its face size: its checks read the face (DECISIONS DS7). With no layout the
    // planner marks the file unclear, so no DIM-* here.
    if st != Subtype::Cubemap && !(ow.is_power_of_two() && oh.is_power_of_two()) {
        add(DimNpot, format!("{ow}x{oh} is not a power of two"));
    }
    if st != Subtype::Cubemap && ow.min(oh) < 4 {
        add(DimSmall, "base dimension below 4 px".into());
    }
    if st == Subtype::Cubemap {
        resized = None;
        if let Some(layout) = cube_layout(p, f) {
            use crate::cube::layout::{face_size, source_cell};
            let src = (a.width, a.height);
            let cell = source_cell(layout, src);
            let face = face_size(p, layout, src);
            let shown =
                if p.drop_top_mip && crate::mips::level_count(face, face, &p.mips.levels) > 1 {
                    face / 2
                } else {
                    face
                };
            if shown != cell {
                resized = Some(
                    Warning::new(DimResized, format!("face {cell} → {shown}"))
                        .with_data(json!({ "from": [cell, cell], "to": [shown, shown] })),
                );
            }
            if shown < 4 {
                add(DimSmall, format!("cube face {shown} px is below 4"));
                // DS20: `allow` cannot acknowledge this one, so no tiny picture cube is written.
                add(
                    CfgInvalid,
                    format!(
                        "cube face {shown} px is below 4; only `sdc cubemap-marker` writes a 1x1 cube"
                    ),
                );
            }
        }
        cube_checks(p, &mut add);
    }
    if st == Subtype::Flow && ow.min(oh) <= 32 {
        add(
            DimFlowSmall,
            "CS ignores _flow maps of 32 px or less".into(),
        );
    }
    // Complex grass (design §3.5, §3.7, §4; rulings R1, R4, R7, R9).
    if st == Subtype::ComplexGrass {
        // The format and tag half is `encoding_warnings`'s; one CFG-INVALID either way.
        if !grass_encoding_bad(p.format, p.color.tag)
            && crate::resize::target_dims((a.width, a.height), p.resize) != (a.width, a.height)
        {
            add(
                CfgInvalid,
                "complex_grass is written as BC7 UNORM at its own size (resize would have to run on each half)"
                    .into(),
            );
        }
        match &a.grass {
            Some(g) if a.is_pot => {
                if g.top_opaque {
                    add(
                        GrassTopOpaque,
                        "top half (diffuse) alpha is fully opaque: the grass has no transparency"
                            .into(),
                    );
                }
                if g.bottom_unit_share < crate::grass::GRASS_UNIT_SHARE {
                    add(
                        GrassNormals,
                        format!(
                            "{:.1}% of bottom-half texels are unit-length normals (|L-1| < 0.1); 99% expected",
                            g.bottom_unit_share * 100.0
                        ),
                    );
                }
                if g.corner_err >= crate::grass::CS_GRASS_THRESHOLD {
                    add(
                        GrassDetectSource,
                        format!(
                            "source bottom-left texel has |L-1| = {:.3} >= 0.03: Community Shaders would not detect \
                             it as complex grass; mip 0 is renormalised, so the written file passes unless \
                             mips.normal_base = keep",
                            g.corner_err
                        ),
                    );
                }
            }
            _ => add(
                GrassShape,
                format!(
                    "{}x{}: complex_grass needs a power-of-two width and height = 2 x width",
                    a.width, a.height
                ),
            ),
        }
    }
    if a.icc_profile {
        add(
            InIccIgnored,
            "embedded ICC profile ignored: values are read as stored (colour content as sRGB)"
                .into(),
        );
    }
    // DECISIONS S6: a re-encoded block-compressed DDS keeps its compression error and gains a second one [SC §7].
    if let Some(src) = a
        .dds_dxgi
        .and_then(crate::dds::from_dxgi)
        .filter(|f| f.block_bytes().is_some())
    {
        let name = match src.as_format() {
            Some((fmt, srgb)) => format!(
                "{}{}",
                fmt.id().to_uppercase(),
                if srgb { "_SRGB" } else { "" }
            ),
            None => format!("{src:?}"),
        };
        add(
            LossReencode,
            format!("compressed DDS ({name}) re-encoded: its compression error is kept and a new one is added; convert the source image if you have it"),
        );
    }
    // DECISIONS Note NV3.
    if p.encoder.backend == Backend::Nvtt {
        use crate::nvtt::{NvttProbe, NVTT_TESTED};
        match nvtt.map(|c| c.get()) {
            Some(NvttProbe::Found(t)) if t.tested() => {}
            Some(NvttProbe::Found(t)) => add(
                EncNvttVersion,
                format!(
                    "NVTT {} at {} is untested ({NVTT_TESTED} is); acknowledge to use it",
                    t.version,
                    t.path.display()
                ),
            ),
            Some(NvttProbe::Missing(why)) => add(
                EncNvttMissing,
                format!("NVTT not found ({why}): install NVTT 3, set the NVTT path, or choose the built-in encoder"),
            ),
            None => add(
                EncNvttMissing,
                "NVTT was not probed for this batch".into(),
            ),
        }
    }
    for c in contradictions(p, f) {
        add(SugContradiction, c);
    }
    w.extend(resized);
    // DECISIONS S9: the count Preprocess will replace: R, G, B always, A only when the alpha is kept.
    let kept_alpha = p.alpha.policy != AlphaPolicy::Remove;
    let samples: u64 =
        a.nonfinite[..3].iter().sum::<u64>() + if kept_alpha { a.nonfinite[3] } else { 0 };
    if samples > 0 {
        w.push(
            Warning::new(
                InNonfinite,
                format!("{samples} NaN or infinite source samples are replaced before processing"),
            )
            .with_data(json!({ "samples": samples })),
        );
    }
    if hdr_outside_cs {
        // Not a "—" cell with a fallback to accept: `--allow CFG-UNAVAILABLE` must not let it through [D#3].
        w.push(Warning {
            severity: Severity::Error,
            ..Warning::new(
                CfgUnavailable,
                if p.format == Format::Bc6hUf16 {
                    "BC6H is usable only under Community Shaders [D#3]; choose another format"
                } else {
                    "RGBA16F is usable only under Community Shaders (DECISIONS S4); choose another format"
                },
            )
        });
    }
    w
}

/// Codes of `warnings` at severity `sev` (SUG-LOW-CONFIDENCE excluded), in first-seen order without repeats.
fn codes_at(warnings: &[Warning], sev: Severity) -> Vec<Code> {
    let mut v: Vec<Code> = vec![];
    for w in warnings
        .iter()
        .filter(|w| w.severity == sev && w.code != Code::SugLowConfidence)
    {
        if !v.contains(&w.code) {
            v.push(w.code);
        }
    }
    v
}

/// Spec §4.4: the job of a raw packing input. Nothing is written; the profile is the built-in `generic_data`
/// placeholder and is never used. It is a raw map waiting for `pack::plan::plan_packing`, which consumes it into a
/// recipe or re-tags it `PACK-UNUSED-INPUT`.
fn packing_input_job(ctx: &ResolveCtx, f: &FileFacts, term: &str, role: RawMap) -> ResolvedJob {
    let profile = ctx.registry.base_profile(ctx.target, Subtype::GenericData);
    let msg = format!(
        "_{term} is a raw {} map: a channel-packing input (spec §6.4), not converted on its own; set a sub-type for it to convert it alone",
        role.id()
    );
    ResolvedJob {
        input: f.input.path.clone(),
        output: f.input.output.clone(),
        game_path: f.input.game_path.clone(),
        set_key: f.set.key.clone(),
        profile,
        provenance: BTreeMap::new(),
        suggestions: vec![],
        warnings: vec![Warning::new(Code::InPackingInput, msg)
            .with_data(json!({ "role": role, "term": term }))],
        status: JobStatus::PackingInput { role },
        suffix_alias: None,
        source: None,
        pack: None,
        bake: None,
        rnm: None,
        derive: None,
        shader: None,
        cube: None,
    }
}

/// M6 Track F2 (spec §2.2): the job of a normal-layer file. Nothing is written; the profile is the built-in
/// `generic_data` placeholder and is never used.
fn normal_layer_job(ctx: &ResolveCtx, f: &FileFacts, term: crate::rnm::LayerTerm) -> ResolvedJob {
    ResolvedJob {
        input: f.input.path.clone(),
        output: f.input.output.clone(),
        game_path: f.input.game_path.clone(),
        set_key: f.set.key.clone(),
        profile: ctx.registry.base_profile(ctx.target, Subtype::GenericData),
        provenance: BTreeMap::new(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::NormalLayer { term: term.into() },
        suffix_alias: None,
        source: None,
        pack: None,
        bake: None,
        rnm: None,
        derive: None,
        shader: None,
        cube: None,
    }
}

/// Resolve one file: sub-type decision, layered profile, §10 checks and status (spec §5.2).
pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    resolve_file_with(ctx, f, false)
}

/// [`resolve_file`] for a file the plugin scanner names as the diffuse of a GRAS model (`gras_diffuse`, complex grass
/// design §5, ruling R11): when nothing explicit chose its sub-type and its analysis passes the detection tests, its
/// top suggestion becomes a high-confidence `complex_grass`, on either target (Note AZ reverses ruling R1).
pub fn resolve_file_with(ctx: &ResolveCtx, f: &FileFacts, gras_diffuse: bool) -> ResolvedJob {
    let mut d = decide_subtype(ctx, f);
    // Spec §4.4: a raw map is a packing input unless a rule or --set names its sub-type.
    if let Some(Vocab {
        term,
        role: VocabRole::PackingInput(role),
    }) = vocab_for(f)
    {
        if !matches!(
            d.provenance,
            Provenance::User | Provenance::Rule(_) | Provenance::ProjectFile
        ) {
            return packing_input_job(ctx, f, term, *role);
        }
    }
    // Note DR: in a set that is Advanced skin by other evidence, `_f` is the fuzz mask (a metal-like grey map), a
    // packing input of the `_rfaos` G; it never makes the set skin itself (`_f` is no `RawMap::INPUTS` member).
    if !matches!(
        d.provenance,
        Provenance::User | Provenance::Rule(_) | Provenance::ProjectFile
    ) && crate::pack::plan::skin_f_candidate(ctx, f.set, f.input)
    {
        return packing_input_job(ctx, f, "f", RawMap::Fuzz);
    }
    // M6 Track F2 (DECISIONS Note W2): a layer file is hidden unless a rule or --set names its sub-type.
    if let Some(Vocab {
        role: VocabRole::NormalLayer(term),
        ..
    }) = vocab_for(f)
    {
        if !matches!(
            d.provenance,
            Provenance::User | Provenance::Rule(_) | Provenance::ProjectFile
        ) {
            return normal_layer_job(ctx, f, *term);
        }
    }
    if gras_diffuse {
        suggest_grass(ctx, f, &mut d);
    }
    resolve_decided(ctx, f, d)
}

/// Design §5: the auto-tag, only over a suggestion (a `--set`, rule, `[files]` entry or set shader has decided already).
fn suggest_grass(ctx: &ResolveCtx, f: &FileFacts, d: &mut SubtypeDecision) {
    if ctx
        .shaders
        .get(&f.set.key)
        .is_some_and(|p| p.provenance == crate::shader::ShaderOrigin::Set)
        || !matches!(d.provenance, Provenance::Suggestion)
        || !crate::grass::looks_like_grass(f.analysis)
    {
        return;
    }
    d.suggestions.insert(
        0,
        Suggestion {
            subtype: Subtype::ComplexGrass,
            confidence: Confidence::High,
            reason: "diffuse of a GRAS model; 1:2; bottom-left texel passes CS's 0.03 test; >= 99% unit normals in the bottom half"
                .into(),
        },
    );
    d.subtype = Subtype::ComplexGrass;
    d.decided = ctx.accept == AcceptSuggestions::High;
}

/// Resolve a file whose sub-type is decided, by `decide_subtype` or by a packing recipe (spec §6.4): layered
/// profile, §10 checks and status.
pub fn resolve_decided(ctx: &ResolveCtx, f: &FileFacts, d: SubtypeDecision) -> ResolvedJob {
    let suffix_alias = alias_use(ctx, f);
    let mut reasons = vec![];
    let mut needs = vec![];
    if !d.decided {
        if !allowed(ctx, f, Code::SugLowConfidence) {
            needs.push(Need::Subtype);
        }
        reasons.push(match d.suggestions.first() {
            Some(s) => format!(
                "sub-type '{}' is a {:?}-confidence suggestion ({})",
                s.subtype.id(),
                s.confidence,
                s.reason
            ),
            None => "no sub-type could be suggested; set one".into(),
        });
    }
    // Spec §4.2: an unavailable sub-type with a fallback is resolved as the fallback when the user accepts it
    // with `--allow CFG-UNAVAILABLE`; one without a fallback stays blocked.
    let mut subtype = d.subtype;
    let mut subtype_prov = d.provenance.clone();
    let mut fallback_note = None;
    if allowed(ctx, f, Code::CfgUnavailable) {
        if let Availability::Unavailable { fallback: Some(fb) } = availability(ctx.target, subtype)
        {
            let msg = format!(
                "{} is unavailable under {:?}; resolved as its fallback {} (--allow CFG-UNAVAILABLE)",
                subtype.id(),
                ctx.target,
                fb.id()
            );
            fallback_note = Some(Warning {
                severity: Severity::Warning,
                ..Warning::new(Code::CfgUnavailable, msg)
                    .with_data(json!({ "from": subtype, "to": fb }))
            });
            subtype = fb;
            subtype_prov = Provenance::Fallback;
        }
    }
    let (mut profile, mut provenance, dropped_filter) =
        match layered_profile_with(ctx, f, subtype, false) {
            Ok(v) => v,
            Err(e) => {
                // Engine::plan (Task 37) validates presets, configs and rules up front, so this path is rare. The
                // placeholder profile comes from `sdc-base`, which user presets cannot replace.
                let fallback = ctx.registry.base_profile(ctx.target, subtype);
                return ResolvedJob {
                    input: f.input.path.clone(),
                    output: f.input.output.clone(),
                    game_path: f.input.game_path.clone(),
                    set_key: f.set.key.clone(),
                    profile: fallback,
                    provenance: BTreeMap::new(),
                    suggestions: d.suggestions,
                    warnings: vec![Warning::new(Code::CfgInvalid, e.to_string())],
                    status: JobStatus::Error {
                        codes: vec![Code::CfgInvalid],
                    },
                    suffix_alias,
                    source: None,
                    pack: None,
                    bake: None,
                    rnm: None,
                    derive: None,
                    shader: None,
                    cube: None,
                };
            }
        };
    // Note CL23: on a single-level chain `drop_top_mip` does nothing, so it is cleared: it stays out of the hash and
    // never re-encodes identical bytes.
    if profile.drop_top_mip {
        let (w, h) =
            crate::resize::target_dims((f.analysis.width, f.analysis.height), profile.resize);
        if crate::mips::level_count(w, h, &profile.mips.levels) <= 1 {
            profile.drop_top_mip = false;
            provenance.remove("drop_top_mip");
        }
    }
    // The category follows the sub-type.
    provenance.insert("subtype".into(), subtype_prov.clone());
    provenance.insert("category".into(), subtype_prov);
    // An unavailable sub-type is blocked by CFG-UNAVAILABLE alone (`check_profile`); the alpha meaning and green
    // convention of a profile that is never written are not asked for.
    let available = availability(profile.target, profile.subtype) == Availability::Available;
    // Spec §3.3/§5.2: a mesh-dependent alpha meaning is unconfirmed unless an explicit layer (a user or project
    // preset, a config, a rule or --set) chose the policy; built-in defaults do not count.
    if available
        && profile.subtype.alpha_from_mesh()
        && profile.subtype != Subtype::ComplexGrass
        && f.analysis.alpha == AlphaContent::Varying
        && !alpha_policy_explicit(ctx, &provenance)
    {
        let hint = suggest_alpha(profile.subtype, f.analysis.alpha, near_binary(f.analysis))
            .map(|(p, _, why)| format!("{p:?}: {why}"));
        if !allowed(ctx, f, Code::SugLowConfidence) {
            needs.push(Need::AlphaMeaning);
        }
        reasons.push(format!(
            "alpha meaning is unconfirmed (proposal {})",
            hint.unwrap_or_default()
        ));
    }
    // DECISIONS Note AW: a plain `_normal`/`_nrm` with no stated green convention is assumed DirectX (no flip). The
    // profile keeps `green_convention` unset, so output bytes and recorded settings are unchanged; only this Info
    // warning says an assumption was made.
    let mut green_note = None;
    if let Some(Vocab {
        term,
        role: VocabRole::Normal(None),
    }) = vocab_for(f)
    {
        if available
            && takes_green(profile.subtype)
            && profile.channels.green_convention.is_none()
            && !profile.channels.flip_green
        {
            green_note = Some(format!(
                "green convention of _{term} is unstated; assumed DirectX (set channels.green_convention=opengl if this is an OpenGL map)"
            ));
        }
    }
    let mut warnings: Vec<Warning> = fallback_note.into_iter().collect();
    warnings.extend(dropped_filter.map(|d| filter_ignored_notice(&profile, f, d)));
    warnings.extend(check_profile(
        &profile,
        f,
        alpha_policy_explicit(ctx, &provenance),
        ctx.nvtt,
    ));
    warnings.extend(green_conflict(ctx, &profile, &provenance));
    if available && profile.subtype == Subtype::SkinWet {
        warnings.push(skin_wet_notice(&profile, f, &provenance));
    }
    if available && profile.subtype == Subtype::PbrEmissive {
        warnings.push(hdr_source_notice(ctx, &profile, f, &provenance));
    }
    if !reasons.is_empty() {
        warnings.push(
            Warning::new(Code::SugLowConfidence, reasons.join("; "))
                .with_data(json!({ "suggestions": d.suggestions })),
        );
    }
    if let Some(r) = green_note {
        warnings.push(Warning::new(Code::InGreenUnstated, r));
    }
    // `--allow` turns a blocking code into a warning. CFG-UNAVAILABLE is acknowledged only by accepting a
    // fallback (above), and warnings that `check_profile` raised to error stay errors.
    for w in &mut warnings {
        if w.severity == Severity::Blocking
            && w.code != Code::CfgUnavailable
            && allowed(ctx, f, w.code)
        {
            w.severity = Severity::Warning;
        }
    }
    let errors = codes_at(&warnings, Severity::Error);
    let blocking = codes_at(&warnings, Severity::Blocking);
    // Precedence: errors, then unresolved (the user must decide first; e.g. an unconfirmed diffuse alpha also
    // triggers LOSS-ALPHA-DATA against the default `remove`), then blocking codes.
    let status = if !errors.is_empty() {
        JobStatus::Error { codes: errors }
    } else if !reasons.is_empty() && !allowed(ctx, f, Code::SugLowConfidence) {
        JobStatus::Unresolved { reasons, needs }
    } else if !blocking.is_empty() {
        JobStatus::Blocked { codes: blocking }
    } else {
        JobStatus::Ready
    };
    ResolvedJob {
        input: f.input.path.clone(),
        output: f.input.output.clone(),
        game_path: f.input.game_path.clone(),
        set_key: f.set.key.clone(),
        profile,
        provenance,
        suggestions: d.suggestions,
        warnings,
        status,
        suffix_alias,
        source: None,
        pack: None,
        bake: None,
        rnm: None,
        derive: None,
        shader: None,
        cube: None,
    }
}
