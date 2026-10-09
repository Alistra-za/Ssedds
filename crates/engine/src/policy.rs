//! Resolve (spec §5.2): merge preset, config, rules, analysis and overrides into an immutable ResolvedJob.
//!
//! Precedence (spec §8, lowest first): built-in (`sdc-base`) → preset chain → user config → project config →
//! rule matches (first match per source, sources in the same order) → analysis facts → `--set` (unscoped, then
//! sub-type-scoped, then file-scoped, each in command-line order).
use crate::analyse::Analysis;
use crate::codes::Code;
use crate::config::*;
use crate::discover::InputFile;
use crate::group::TextureSet;
use crate::image::SourceInfo;
use crate::overrides::{to_patch, Scope, SetOverride};
pub use crate::presets::Provenance;
use crate::presets::*;
use crate::suggest::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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
    pub suffix: Option<&'a str>,
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

/// Case-insensitive glob over the game path or the input-relative path.
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
    m.is_match(&f.input.game_path) || m.is_match(&rel)
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

/// Rule sources in precedence order: the preset chain (base first), user config, project config.
/// `preset` is the file's preset, a parameter so a per-file preset can stand in for the batch preset.
fn rule_sources<'a>(ctx: &'a ResolveCtx, preset: &str) -> Vec<(String, &'a [Rule])> {
    let mut v: Vec<(String, &[Rule])> = vec![];
    if let Ok(chain) = ctx.registry.chain(preset) {
        for p in chain {
            v.push((p.id.clone(), &p.rules));
        }
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
    let preset = ctx.preset;
    let suggestions = suggest(&SuggestContext {
        target: ctx.target,
        suffix: f.suffix,
        game_path: &f.input.game_path,
        set_suffixes: &f.set.suffixes,
        alpha: f.analysis.alpha,
        is_float: f.info.is_float,
    });
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
    for (src, rules) in rule_sources(ctx, preset).into_iter().rev() {
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

fn apply(
    doc: &mut Value,
    prov: &mut BTreeMap<String, Provenance>,
    patch: &Value,
    source: Provenance,
) {
    let mut paths = vec![];
    leaf_paths(patch, "", &mut paths);
    merge_json(doc, patch);
    for p in paths {
        prov.insert(p, source.clone());
    }
}

fn config_patches(c: &ConfigFile, st: Subtype) -> Vec<Value> {
    let mut v = vec![c.defaults.clone()];
    if let Some(s) = c.subtypes.get(&st) {
        v.push(s.clone());
    }
    v
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
    let preset = ctx.preset;
    let mut doc = Value::Object(Map::new());
    let mut prov = BTreeMap::new();
    for (source, patch) in ctx.registry.layers(preset, st)? {
        apply(&mut doc, &mut prov, &patch, source);
    }
    for (cfg, source) in [
        (ctx.user_config, Provenance::UserConfig),
        (ctx.project_config, Provenance::ProjectConfig),
    ] {
        if let Some(c) = cfg {
            for patch in config_patches(c, st) {
                apply(&mut doc, &mut prov, &patch, source.clone());
            }
        }
    }
    for (src, rules) in rule_sources(ctx, preset) {
        if let Some((i, r)) = first_match(rules, f) {
            apply(
                &mut doc,
                &mut prov,
                &r.overrides,
                Provenance::Rule(format!("{src}#{i}")),
            );
        }
    }
    let policy_is_default = matches!(prov.get("alpha.policy"), None | Some(Provenance::Builtin));
    if policy_is_default {
        if let Some((policy, Confidence::High, _)) =
            suggest_alpha(st, f.analysis.alpha, near_binary(f.analysis))
        {
            let value = serde_json::to_value(policy).expect("AlphaPolicy serialises");
            apply(
                &mut doc,
                &mut prov,
                &to_patch("alpha.policy", &value),
                Provenance::Analysis,
            );
        }
    }
    for o in ranked_overrides(ctx)
        .into_iter()
        .filter(|o| o.field != "subtype")
    {
        if override_applies(o, Some(st), f) {
            apply(
                &mut doc,
                &mut prov,
                &to_patch(&o.field, &o.value),
                Provenance::User,
            );
        }
    }
    let profile = finish_profile(doc, ctx.target, st)?;
    Ok((profile, prov))
}

/// Share of pixels whose alpha is ≤ 5 or ≥ 250.
pub fn near_binary(a: &Analysis) -> f64 {
    let n = (a.width as f64 * a.height as f64).max(1.0);
    (a.alpha_hist.le5 + a.alpha_hist.ge250) as f64 / n
}
