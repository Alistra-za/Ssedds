//! Resolve (spec §5.2): merge preset, config, rules, analysis and overrides into an immutable ResolvedJob.
//!
//! Precedence (spec §8, lowest first): built-in (`sdc-base`) → preset chain → user config → project config →
//! rule matches (first match per source, sources in the same order) → analysis facts → `--set` (unscoped, then
//! sub-type-scoped, then file-scoped, each in command-line order).
use crate::analyse::Analysis;
use crate::codes::{Code, Severity, Warning};
use crate::config::*;
use crate::discover::InputFile;
use crate::group::{vocab_of, TextureSet, Vocab, VocabRole};
use crate::image::SourceInfo;
use crate::overrides::{to_patch, Scope, SetOverride};
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
}

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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
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

/// Case-insensitive glob over the game path or the input-relative path. A glob without '/' also matches the
/// file name alone, so `file:*.png` and `file:wood01.png` work in any folder.
fn glob_match(glob: &str, f: &FileFacts) -> bool {
    let Ok(g) = globset::GlobBuilder::new(&glob.to_ascii_lowercase())
        .literal_separator(true)
        .build()
    else {
        return false;
    };
    let m = g.compile_matcher();
    let rel = f
        .input
        .rel
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let name = rel.rsplit('/').next().unwrap_or_default();
    m.is_match(&f.input.game_path) || m.is_match(&rel) || (!glob.contains('/') && m.is_match(name))
}

pub fn rule_matches(m: &RuleMatch, f: &FileFacts) -> bool {
    m.glob.as_deref().is_none_or(|g| glob_match(g, f))
        && m.suffix
            .as_deref()
            .is_none_or(|s| f.suffix.unwrap_or("").eq_ignore_ascii_case(s))
        && m.set_has
            .as_deref()
            .is_none_or(|s| f.set.suffixes.contains(&s.to_ascii_lowercase()))
        && m.alpha.is_none_or(|a| a == f.analysis.alpha)
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

fn first_match<'a>(rules: &'a [Rule], f: &FileFacts) -> Option<(usize, &'a Rule)> {
    rules
        .iter()
        .enumerate()
        .find(|(_, r)| rule_matches(&r.matcher, f))
}

fn override_applies(o: &SetOverride, st: Option<Subtype>, f: &FileFacts) -> bool {
    match &o.scope {
        Scope::All => true,
        Scope::Subtype(s) => Some(*s) == st,
        Scope::File(g) => glob_match(g, f),
    }
}

/// `--set` overrides, lowest precedence first: unscoped, then sub-type-scoped, then file-scoped, each group in
/// command-line order (the sort is stable).
fn ranked_overrides<'a>(ctx: &ResolveCtx<'a>) -> Vec<&'a SetOverride> {
    let mut v: Vec<&SetOverride> = ctx.overrides.iter().collect();
    v.sort_by_key(|o| match o.scope {
        Scope::All => 0,
        Scope::Subtype(_) => 1,
        Scope::File(_) => 2,
    });
    v
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
    let set_subtype = ranked_overrides(ctx)
        .into_iter()
        .rev()
        .find(|o| o.field == "subtype" && override_applies(o, None, f));
    if let Some(o) = set_subtype {
        if let Ok(st) = serde_json::from_value::<Subtype>(o.value.clone()) {
            return SubtypeDecision {
                subtype: st,
                provenance: Provenance::User,
                decided: true,
                suggestions,
            };
        }
    }
    // rules: highest-precedence source with a matching rule that names a sub-type
    for (src, rules) in rule_sources(ctx, &chain).into_iter().rev() {
        if let Some((i, r)) = first_match(rules, f) {
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

/// Which patch last set `format` and `encoder.backend`, and how specifically.
#[derive(Debug, Default)]
struct FormatSources {
    patches: usize,
    format: Option<(Reach, usize)>,
    backend: Option<(Reach, usize)>,
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
    match prov.get("alpha.policy") {
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
    // The alias preset's chain for an aliased file (spec §3.7), else the batch preset's.
    let chain = preset_chain(ctx, f)?;
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
    for (src, rules) in rule_sources(ctx, &chain) {
        if let Some((i, r)) = first_match(rules, f) {
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
        .filter(|o| o.field != "subtype")
    {
        if override_applies(o, Some(st), f) {
            let reach = match o.scope {
                Scope::All => GENERIC,
                Scope::Subtype(_) => SUBTYPE_SET,
                Scope::File(_) => FILE,
            };
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch(&o.field, &o.value),
                Provenance::User,
                reach,
            );
        }
    }
    let mut profile = finish_profile(doc, ctx.target, st)?;
    backend_follows_format(&mut profile, &mut prov, &sources);
    // Spec §3.2, §5.4: a stated OpenGL convention is flipped to DirectX in Preprocess (Task 28 reads flip_green).
    if takes_green(st) && profile.channels.green_convention == Some(GreenConvention::Opengl) {
        profile.channels.flip_green = true;
        if let Some(src) = prov.get("channels.green_convention").cloned() {
            prov.insert("channels.flip_green".into(), src);
        }
    }
    Ok((profile, prov))
}

/// Whether `b` can write format `f`.
fn backend_fits(b: Backend, f: Format) -> bool {
    match b {
        Backend::Bc7e => f == Format::Bc7,
        Backend::Rgbcx => matches!(f, Format::Bc1 | Format::Bc3 | Format::Bc4 | Format::Bc5),
        Backend::IspcBc6h => f == Format::Bc6hUf16,
        Backend::Nvtt => matches!(f, Format::Bc7 | Format::Bc6hUf16),
    }
}

/// The built-in encoder for a format (spec §3.2 `encoder.backend` default "Format"); never NVTT.
pub(crate) fn default_backend(f: Format) -> Backend {
    match f {
        Format::Bc1 | Format::Bc3 | Format::Bc4 | Format::Bc5 => Backend::Rgbcx,
        Format::Bc6hUf16 => Backend::IspcBc6h,
        Format::Bc7 => Backend::Bc7e,
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
        (_, PbrSubsurface | PbrFuzz | CoatNormal | Rmaos | Rfaos | Flow | HdrSky) => un(None),
        (Target::Vanilla, ComplexMaterial) => un(Some(Envmask)),
        _ => Availability::Available,
    }
}

/// What happens to a file (spec §5.2, §10). Serialises as `{"state": "ready"}`,
/// `{"state": "blocked", "codes": [...]}` and so on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Ready,
    /// A sub-type or alpha meaning still needs a decision (rule, flag or accepted high-confidence suggestion).
    Unresolved {
        reasons: Vec<String>,
    },
    /// A raw single-channel map (spec §4.4): a channel-packing input (§6.4, milestone M2.5), not converted on
    /// its own and not an error.
    PackingInput {
        role: RawMap,
    },
    /// Blocking codes not acknowledged with `--allow`.
    Blocked {
        codes: Vec<Code>,
    },
    Error {
        codes: Vec<Code>,
    },
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
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
}

/// Formats that store alpha in v1. BC1 is opaque only (spec §3.3, DECISIONS Note F), although
/// `Format::has_alpha` counts its 1-bit mode.
fn stores_alpha(f: Format) -> bool {
    matches!(f, Format::Bc3 | Format::Bc7)
}

/// `normal_len_err` above this is "far from unit length" (CHAN-NORMAL-LENGTH, SUG-CONTRADICTION).
const NORMAL_LEN_TOL: f64 = 0.1;

/// Spec §4.3 rule 5: content that contradicts the decided sub-type. Reported, never blocking.
fn contradictions(p: &Profile, f: &FileFacts) -> Vec<String> {
    let a = f.analysis;
    let st = p.subtype;
    let mut v = vec![];
    if p.category == Category::Normal {
        if a.grayscale {
            v.push(format!("{} source is grayscale (R = G = B)", st.id()));
        }
        if st != Subtype::Msn && a.normal_len_err > NORMAL_LEN_TOL && !p.channels.reconstruct_z {
            v.push(format!(
                "{} source does not decode to unit normals (mean |length-1| = {:.3})",
                st.id(),
                a.normal_len_err
            ));
        }
    }
    if st == Subtype::ComplexMaterial && a.alpha != AlphaContent::Varying {
        v.push("complex_material without varying alpha: this reads as a plain envmask".into());
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

/// The §10 checks that Resolve can make from the profile, the file's path and its analysis.
pub fn check_profile(p: &Profile, f: &FileFacts) -> Vec<Warning> {
    use Code::*;
    let a = f.analysis;
    let mut w = vec![];
    let mut add = |c: Code, m: String| w.push(Warning::new(c, m));
    let st = p.subtype;
    if let Availability::Unavailable { fallback } = availability(p.target, st) {
        let fix = match fallback {
            Some(fb) => format!(
                "accept the fallback {} with --allow CFG-UNAVAILABLE",
                fb.id()
            ),
            None => "no fallback; change the sub-type or the target".to_string(),
        };
        add(
            CfgUnavailable,
            format!("{} is unavailable under {:?}; {fix}", st.id(), p.target),
        );
    }
    // BC6H outside cs cannot be acknowledged: an error, added after the other checks.
    let bc6h_outside_cs = p.format == Format::Bc6hUf16 && p.target != Target::Cs;
    if !(5..=200).contains(&p.alpha.threshold) {
        add(
            CfgInvalid,
            format!("alpha.threshold {} outside 5..=200", p.alpha.threshold),
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
    if let Some(s) = &p.channels.swizzle {
        if s.len() != 4 || !s.chars().all(|c| "rgba01".contains(c)) {
            add(
                CfgInvalid,
                format!("channels.swizzle '{s}' must be 4 of r,g,b,a,0,1"),
            );
        }
    }
    // BC6H has no _SRGB or UNORM form, and every other format needs one (spec §3.2, §4.2).
    let bc6h = p.format == Format::Bc6hUf16;
    if bc6h != (p.color.tag == ColorTag::NotApplicable) {
        let msg = if bc6h {
            "BC6H has no _SRGB or UNORM form; set color.tag=n/a".to_string()
        } else if st.is_pbr_color() && p.target == Target::Cs {
            format!(
                "color.tag n/a is only for BC6H on {}: set color.tag=srgb for a non-BC6H _g",
                st.id()
            )
        } else {
            format!(
                "color.tag n/a is only for BC6H: set color.tag=unorm for a non-BC6H {}",
                st.id()
            )
        };
        add(CfgInvalid, msg);
    }
    let srgb = p.srgb_tag();
    if srgb && p.target != Target::Cs {
        add(
            TagSrgbVanilla,
            "_SRGB is not usable under vanilla or CPM shaders [D#4]".into(),
        );
    } else if srgb && !st.is_pbr_color() {
        add(
            TagSrgbNonpbr,
            "_SRGB on a non-PBR sub-type renders darker [SC §6]".into(),
        );
    }
    if p.color.tag == ColorTag::Unorm && st.is_pbr_color() && !bc6h && p.target == Target::Cs {
        add(
            TagUnormPbr,
            "UNORM on PBR colour looks washed out with linear lighting off".into(),
        );
    }
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
    if p.category == Category::Normal && matches!(p.format, Format::Bc4 | Format::Bc5) {
        add(
            FmtBc5Normal,
            "BC4/BC5 on a normal: Skyrim reads Z directly, so Z decodes -1 and alpha is lost [SC §2]".into(),
        );
    }
    if st == Subtype::LandscapeDiffuse && p.format == Format::Bc1 {
        add(
            FmtLandscapeBc1,
            "BC1 destroys the landscape snow mask / height alpha".into(),
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
    if p.encoder.perceptual
        && !(p.category == Category::Color && p.alpha.policy != AlphaPolicy::Data)
    {
        add(
            FmtPerceptualData,
            "perceptual BC7 is allowed only for colour without data alpha [B2 §1]".into(),
        );
    }
    if a.alpha == AlphaContent::Varying
        && (p.alpha.policy == AlphaPolicy::Remove || !stores_alpha(p.format))
    {
        add(
            LossAlphaData,
            "non-constant alpha would be discarded".into(),
        );
    }
    if (f.info.bit_depth > 8 || f.info.is_float) && !bc6h {
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
    if p.alpha.policy == AlphaPolicy::Data && a.alpha == AlphaContent::Constant255 {
        add(
            ChanConstantAlphaData,
            "data alpha is constant 255; remove would save bits".into(),
        );
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
        && stores_alpha(p.format)
        && !p.alpha.exact_opaque
        && p.alpha.policy != AlphaPolicy::Remove
    {
        add(
            AlphaExactOffM,
            "exact opaque alpha is off on an _m that stores alpha; 254 may reach PGPatcher/ENB"
                .into(),
        );
    }
    if !a.is_pot {
        add(
            DimNpot,
            format!("{}x{} is not a power of two", a.width, a.height),
        );
    }
    if a.width.min(a.height) < 4 {
        add(DimSmall, "base dimension below 4 px".into());
    }
    if st == Subtype::Flow && a.width.min(a.height) <= 32 {
        add(
            DimFlowSmall,
            "CS ignores _flow maps of 32 px or less".into(),
        );
    }
    if p.encoder.backend == Backend::Nvtt {
        add(
            EncNvttMissing,
            "the NVTT backend arrives in milestone M5".into(),
        );
    }
    for c in contradictions(p, f) {
        add(SugContradiction, c);
    }
    if bc6h_outside_cs {
        // Not a "—" cell with a fallback to accept: `--allow CFG-UNAVAILABLE` must not let it through [D#3].
        w.push(Warning {
            severity: Severity::Error,
            ..Warning::new(
                CfgUnavailable,
                "BC6H is usable only under Community Shaders [D#3]; choose another format",
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
/// placeholder and is never used. Milestone M2.5 consumes these files in packing recipes.
fn packing_input_job(ctx: &ResolveCtx, f: &FileFacts, term: &str, role: RawMap) -> ResolvedJob {
    let profile = ctx
        .registry
        .profile(BASE_ID, ctx.target, Subtype::GenericData)
        .expect("sdc-base profile is valid");
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
    }
}

/// Resolve one file: sub-type decision, layered profile, §10 checks and status (spec §5.2).
pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    let suffix_alias = alias_use(ctx, f);
    let d = decide_subtype(ctx, f);
    // Spec §4.4: a raw map is a packing input unless a rule or --set names its sub-type.
    if let Some(Vocab {
        term,
        role: VocabRole::PackingInput(role),
    }) = vocab_for(f)
    {
        if !matches!(d.provenance, Provenance::User | Provenance::Rule(_)) {
            return packing_input_job(ctx, f, term, *role);
        }
    }
    let mut reasons = vec![];
    if !d.decided {
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
    if ctx.allow.contains(&Code::CfgUnavailable) {
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
    let (profile, mut provenance) = match layered_profile(ctx, f, subtype) {
        Ok(v) => v,
        Err(e) => {
            // The built-in base preset is valid by construction (Task 20 test), so this cannot fail.
            // Engine::plan (Task 37) validates presets, configs and rules up front, so this path is rare.
            let fallback = ctx
                .registry
                .profile(BASE_ID, ctx.target, subtype)
                .expect("sdc-base profile is valid");
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
            };
        }
    };
    // The category follows the sub-type.
    provenance.insert("subtype".into(), subtype_prov.clone());
    provenance.insert("category".into(), subtype_prov);
    // Spec §3.3/§5.2: a mesh-dependent alpha meaning is unconfirmed unless an explicit layer (a user or project
    // preset, a config, a rule or --set) chose the policy; built-in defaults do not count.
    if profile.subtype.alpha_from_mesh()
        && f.analysis.alpha == AlphaContent::Varying
        && !alpha_policy_explicit(ctx, &provenance)
    {
        let hint = suggest_alpha(profile.subtype, f.analysis.alpha, near_binary(f.analysis))
            .map(|(p, _, why)| format!("{p:?}: {why}"));
        reasons.push(format!(
            "alpha meaning is unconfirmed (proposal {})",
            hint.unwrap_or_default()
        ));
    }
    // Spec §4.4 (DECISIONS Note H): a plain `_normal`/`_nrm` has no default green convention. It is not a suggestion,
    // so neither --accept-suggestions high nor --allow SUG-LOW-CONFIDENCE settles it.
    let mut green_unstated = false;
    if let Some(Vocab {
        term,
        role: VocabRole::Normal(None),
    }) = vocab_for(f)
    {
        if takes_green(profile.subtype) && profile.channels.green_convention.is_none() {
            green_unstated = true;
            reasons.push(format!(
                "green convention of _{term} is unstated: set channels.green_convention=opengl or directx (preset, config, rule or --set)"
            ));
        }
    }
    let mut warnings: Vec<Warning> = fallback_note.into_iter().collect();
    warnings.extend(check_profile(&profile, f));
    if !reasons.is_empty() {
        warnings.push(
            Warning::new(Code::SugLowConfidence, reasons.join("; "))
                .with_data(json!({ "suggestions": d.suggestions })),
        );
    }
    // `--allow` turns a blocking code into a warning. CFG-UNAVAILABLE is acknowledged only by accepting a
    // fallback (above), and warnings that `check_profile` raised to error stay errors.
    for w in &mut warnings {
        if w.severity == Severity::Blocking
            && w.code != Code::CfgUnavailable
            && ctx.allow.contains(&w.code)
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
    } else if (!reasons.is_empty() && !ctx.allow.contains(&Code::SugLowConfidence))
        || green_unstated
    {
        JobStatus::Unresolved { reasons }
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
    }
}
