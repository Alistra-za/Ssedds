//! Plan-time wiring of channel packing (spec §6.4, plan M2.5, DECISIONS Note M). `Engine::plan` calls
//! [`plan_packing`] once every file is resolved (pass 1: recipes, packing settings, conflicts, consumed and unused
//! raw jobs), then assembles, analyses and resolves each [`Candidate`] with [`finish_recipe`] (pass 2).
use super::assemble::{check_sizes, sizes, PackError};
use super::{
    file_name as name, recipes_for_with, resolve_channels, ChannelSource, PackJob, PackSource,
    RawInput, RecipeKind, Slot,
};
use crate::analyse::{analyse, Analysis};
use crate::api::SkippedFile;
use crate::codes::{Code, Warning};
use crate::config::{GraySource, PackPrefer, PackRecipe, Profile, RawMap, Target};
use crate::discover::{InputFile, InputKind};
use crate::events::{RecipeReport, RecipeStatus};
use crate::group::{split_suffix_with, split_vocab, TextureSet, VocabRole};
use crate::image::{Rgba32F, SourceInfo};
use crate::policy::{
    layered_profile_placeholder, resolve_decided, FileFacts, JobStatus, ResolveCtx, ResolvedJob,
    SourceStamp, SubtypeDecision,
};
use crate::presets::Provenance;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const CHANNELS: [&str; 4] = ["R", "G", "B", "A"];

// `name` is `pack::file_name` (imported above), the file-name text used in every message of this module.

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| name(p)).collect::<Vec<_>>().join(", ")
}

/// The raw-map role of an input file, from its name (spec §4.4).
pub fn raw_role(f: &InputFile) -> Option<RawMap> {
    if f.kind != InputKind::Image {
        return None;
    }
    let stem = f.rel.file_stem()?.to_string_lossy().into_owned();
    match split_vocab(&stem)?.1.role {
        VocabRole::PackingInput(r) => Some(r),
        _ => None,
    }
}

/// The packed output as an input file: the first source's `path` (so `job.input` is a real file, Note M8), with
/// the output's name `<base>_<suffix>.<ext>` in `rel` and `game_path` (rule globs and `file:` scopes match it)
/// and the packed `output` next to the source's.
pub fn virtual_input(first: &InputFile, kind: RecipeKind) -> InputFile {
    let stem = virtual_stem(&first.rel, kind);
    let ext = first
        .rel
        .extension()
        .map_or_else(|| "png".to_string(), |e| e.to_string_lossy().into_owned());
    let file = format!("{stem}.{ext}");
    let lower = file.to_lowercase();
    let game_path = match first.game_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{lower}"),
        None => lower,
    };
    InputFile {
        path: first.path.clone(),
        rel: first.rel.with_file_name(&file),
        game_path,
        output: first.output.with_file_name(format!("{stem}.dds")),
        kind: InputKind::Image,
    }
}

/// The packed output's stem, from its first source's name: `<base>_<suffix>` (`rock01_roughness` → `rock01_rmaos`).
/// Its `[files]` key is this stem in the first source's folder, wherever an output path puts the file.
pub fn virtual_stem(first_source: &Path, kind: RecipeKind) -> String {
    let stem = first_source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let base = split_vocab(&stem).map_or(stem.clone(), |(b, _)| b);
    format!("{base}_{}", kind.suffix())
}

/// A recipe that passed pass 1: assembled, analysed and resolved in pass 2.
pub struct Candidate {
    pub set_key: String,
    pub kind: RecipeKind,
    pub virtual_input: InputFile,
    /// Carries `gray`, the `channels.grayscale_source` every later step uses (Note M6).
    pub job: PackJob,
    /// Channels that took a neutral default (index, slot): one `PACK-DEFAULTED` each.
    pub defaulted: Vec<(usize, Slot)>,
}

pub struct PackPlan {
    pub candidates: Vec<Candidate>,
    /// Recipe reports per set key, for `set_resolved`.
    pub reports: BTreeMap<String, Vec<RecipeReport>>,
}

/// Marks the report of `c` `Failed` with `reason`: pass 2 could not plan it (a load or assembly failure, a panic, or
/// a cancel before its sources were loaded; DECISIONS Note M11), so `set_resolved` must not say it runs.
pub fn fail_report(
    reports: &mut BTreeMap<String, Vec<RecipeReport>>,
    c: &Candidate,
    reason: String,
) {
    let output = slash(&c.virtual_input.output);
    if let Some(r) = reports
        .get_mut(&c.set_key)
        .and_then(|v| v.iter_mut().find(|r| r.output == output))
    {
        r.status = RecipeStatus::Failed;
        r.reason = Some(reason);
    }
}

/// The analysis pass 1 resolves a packed profile with: the packed image does not exist yet (DECISIONS Note M6).
/// Pass 1 ignores rules that match on `alpha` (`layered_profile_placeholder`), so its alpha never decides anything.
fn placeholder() -> (Analysis, SourceInfo) {
    let info = SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: true,
        is_float: false,
    };
    (analyse(&Rgba32F::new(4, 4, [0.5; 4]), &info), info)
}

/// Replaces a waiting raw job's packing-input warning (`policy::packing_input_job` gives it exactly one).
fn retag(jobs: &mut [ResolvedJob], path: &Path, code: Code, message: String) {
    if let Some(j) = jobs
        .iter_mut()
        .find(|j| j.input == path && matches!(j.status, JobStatus::PackingInput { .. }))
    {
        let data = j
            .warnings
            .first()
            .map(|w| w.data.clone())
            .unwrap_or_default();
        j.warnings = vec![Warning::new(code, message).with_data(data)];
    }
}

/// A planned non-raw image member that already provides `kind`'s output (spec §6.4 "Pre-packed conflict",
/// DECISIONS Note M9).
fn prepacked_member<'a>(
    ctx: &ResolveCtx,
    set: &TextureSet,
    files: &'a [InputFile],
    jobs: &[ResolvedJob],
    kind: RecipeKind,
) -> Option<&'a InputFile> {
    set.members.iter().map(|&i| &files[i]).find(|f| {
        let stem = f
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        f.kind == InputKind::Image
            && raw_role(f).is_none()
            && split_suffix_with(&stem, ctx.registry.alias_table())
                .suffix
                .as_deref()
                == Some(kind.suffix())
            && jobs.iter().any(|j| j.input == f.path)
    })
}

/// A recipe whose sources differ in size (spec §6.4 "Dimensions"): an error job, so `run` fails it. Its sub-type
/// and category come from the recipe (`recipe:<kind>`), as for a recipe job that runs; the caller adds
/// `provenance["target"]`, as for every job.
fn size_mismatch_job(
    ctx: &ResolveCtx,
    vin: &InputFile,
    set: &TextureSet,
    kind: RecipeKind,
    found: &[(PathBuf, u32, u32)],
) -> ResolvedJob {
    let text = sizes(found);
    let data: Vec<_> = found
        .iter()
        .map(|(p, w, h)| json!({ "path": slash(p), "width": w, "height": h }))
        .collect();
    let recipe = Provenance::Recipe(kind.id().into());
    ResolvedJob {
        input: vin.path.clone(),
        output: vin.output.clone(),
        game_path: vin.game_path.clone(),
        set_key: set.key.clone(),
        profile: ctx.registry.base_profile(ctx.target, kind.subtype()),
        provenance: BTreeMap::from([
            ("subtype".to_string(), recipe.clone()),
            ("category".to_string(), recipe),
        ]),
        suggestions: vec![],
        warnings: vec![Warning::new(
            Code::PackSizeMismatch,
            format!(
                "sources of {} differ in size: {text}; no resizing in v1",
                name(&vin.output)
            ),
        )
        .with_data(json!({ "sources": data }))],
        status: JobStatus::Error {
            codes: vec![Code::PackSizeMismatch],
        },
        suffix_alias: None,
        source: None,
        pack: None,
    }
}

/// The packed output's pass 1 profile (DECISIONS Note M6): its `packing.*` and `channels.grayscale_source`.
fn pass1_profile(
    ctx: &ResolveCtx,
    set: &TextureSet,
    vin: &InputFile,
    kind: RecipeKind,
    pa: &Analysis,
    pinfo: &SourceInfo,
) -> Profile {
    let facts = FileFacts {
        input: vin,
        suffix: Some(kind.suffix()),
        alias: None,
        set,
        analysis: pa,
        info: pinfo,
    };
    layered_profile_placeholder(ctx, &facts, kind.subtype())
        .map(|(p, _)| p)
        .unwrap_or_else(|_| ctx.registry.base_profile(ctx.target, kind.subtype()))
}

/// The set's `packing.recipe` (cs only; DECISIONS Note R-fills-1). The packed output's `[files]` key carries its
/// suffix, which is what the recipe decides, so the setting is read from the key of the output it names: the
/// `_rfaos` entry saying `rfaos` (checked first) or the `_rmaos` entry saying `rmaos`. A project, preset or
/// default value applies to both keys alike. Anything else is `auto`.
fn chosen_recipe(
    ctx: &ResolveCtx,
    set: &TextureSet,
    members: &BTreeMap<PathBuf, &InputFile>,
    raw: &[RawInput],
    pa: &Analysis,
    pinfo: &SourceInfo,
) -> PackRecipe {
    if ctx.target != Target::Cs {
        return PackRecipe::Auto;
    }
    for pick in [PackRecipe::Rfaos, PackRecipe::Rmaos] {
        let (recipes, _) = recipes_for_with(ctx.target, raw, pick);
        let Some(r) = recipes
            .iter()
            .find(|r| matches!(r.kind, RecipeKind::Rfaos | RecipeKind::Rmaos))
        else {
            continue;
        };
        let first = &r.sources()[0].0;
        let vin = virtual_input(members[first], r.kind);
        if pass1_profile(ctx, set, &vin, r.kind, pa, pinfo)
            .packing
            .recipe
            == pick
        {
            return pick;
        }
    }
    PackRecipe::Auto
}

/// Pass 1 over every set. `jobs` holds every resolved job of the batch and is changed in place: consumed raw jobs
/// are removed, unused ones re-tagged `PACK-UNUSED-INPUT` (`IN-PACKING-INPUT` when their recipe is disabled), a
/// pre-packed job gets `PACK-CONFLICT` or (with `packing.prefer = raw`) moves to `skipped`, and a recipe whose
/// sources differ in size becomes an error job. `failed` holds the paths that failed planning and `cancelled` those
/// whose Load never started because the batch was cancelled: a recipe with a member in either is `Failed` (Note M11).
/// `packed_outputs` holds explicit GUI output paths of packed outputs by (set key, suffix), M3b.
#[allow(clippy::too_many_arguments)]
pub fn plan_packing(
    ctx: &ResolveCtx,
    sets: &[TextureSet],
    files: &[InputFile],
    jobs: &mut Vec<ResolvedJob>,
    failed: &[PathBuf],
    cancelled: &[PathBuf],
    skipped: &mut Vec<SkippedFile>,
    packed_outputs: &BTreeMap<(String, String), PathBuf>,
) -> PackPlan {
    let (pa, pinfo) = placeholder();
    let mut out = PackPlan {
        candidates: vec![],
        reports: BTreeMap::new(),
    };
    let mut consumed: BTreeSet<PathBuf> = BTreeSet::new();
    for set in sets {
        // Raw members still waiting for a recipe: a planned packing input, or one that failed to load or was
        // cancelled before its Load. A raw map whose sub-type a rule or `--set` named is an ordinary file
        // (spec §4.4) and takes no part.
        let mut raw = vec![];
        let mut members: BTreeMap<PathBuf, &InputFile> = BTreeMap::new();
        for &i in &set.members {
            let f = &files[i];
            let Some(role) = raw_role(f) else { continue };
            let waiting = jobs
                .iter()
                .any(|j| j.input == f.path && matches!(j.status, JobStatus::PackingInput { .. }));
            if waiting || failed.contains(&f.path) || cancelled.contains(&f.path) {
                raw.push(RawInput {
                    path: f.path.clone(),
                    role,
                });
                members.insert(f.path.clone(), f);
            }
        }
        if raw.is_empty() {
            continue;
        }
        let choice = chosen_recipe(ctx, set, &members, &raw, &pa, &pinfo);
        let (recipes, unused) = recipes_for_with(ctx.target, &raw, choice);
        let mut unused: Vec<(PathBuf, String)> =
            unused.into_iter().map(|u| (u.path, u.reason)).collect();
        let mut reports = vec![];
        for recipe in recipes {
            let sources: Vec<PathBuf> = recipe.sources().into_iter().map(|(p, _)| p).collect();
            let mut vin = virtual_input(members[&sources[0]], recipe.kind);
            if let Some(p) =
                packed_outputs.get(&(set.key.clone(), recipe.kind.suffix().to_string()))
            {
                vin.output = p.clone();
            }
            let st = recipe.kind.subtype();
            // Pass 1: the packed output's own profile decides `packing.*` (DECISIONS Note M6).
            let profile = pass1_profile(ctx, set, &vin, recipe.kind, &pa, &pinfo);
            let (packing, gray) = (profile.packing, profile.channels.grayscale_source);
            let (channels, defaulted) = resolve_channels(&recipe, &packing);
            let mut report = RecipeReport {
                output: slash(&vin.output),
                subtype: st,
                channels: channels
                    .iter()
                    .map(|c| {
                        c.provenance(gray)
                            .map(|p| p.to_string())
                            .unwrap_or_default()
                    })
                    .collect(),
                status: RecipeStatus::Run,
                reason: None,
            };
            // DECISIONS Note M11: a source that failed to load fails the recipe. So does one whose Load was
            // cancelled: the report must not claim `default:` channels for sources that exist.
            if let Some(bad) = sources.iter().find(|p| failed.contains(p)) {
                report.status = RecipeStatus::Failed;
                report.reason = Some(format!("source {} could not be read", name(bad)));
                reports.push(report);
                consumed.extend(sources.iter().cloned());
                continue;
            }
            if sources.iter().any(|p| cancelled.contains(p)) {
                report.status = RecipeStatus::Failed;
                report.reason = Some("cancelled".into());
                reports.push(report);
                consumed.extend(sources.iter().cloned());
                continue;
            }
            if !packing.enabled {
                report.status = RecipeStatus::Disabled;
                report.reason = Some("packing.enabled = false".into());
                reports.push(report);
                for p in &sources {
                    retag(
                        jobs,
                        p,
                        Code::InPackingInput,
                        format!(
                            "{}: packing is off for {} (packing.enabled = false); a raw map is not converted on its own",
                            name(p),
                            name(&vin.output)
                        ),
                    );
                }
                continue;
            }
            // Headers only. Checked before the conflict below so that a recipe which cannot be packed never
            // replaces the pre-packed file the user had (DECISIONS Note M9: never write neither).
            let mismatch = match check_sizes(&sources) {
                Err(PackError::SizeMismatch(found)) => Some(found),
                _ => None,
            };
            let mut kept = None;
            if let Some(pre) = prepacked_member(ctx, set, files, jobs, recipe.kind) {
                let pre_output = pre.output.clone();
                let pre = pre.path.clone();
                report.status = RecipeStatus::Conflict;
                if packing.prefer == PackPrefer::Prepacked {
                    report.reason = Some(format!(
                        "{} is used (packing.prefer = prepacked)",
                        name(&pre)
                    ));
                    reports.push(report);
                    if let Some(j) = jobs.iter_mut().find(|j| j.input == pre) {
                        j.warnings.push(Warning::new(
                            Code::PackConflict,
                            format!(
                                "{} and the raw maps {} both make {}; the pre-packed file is used (packing.prefer = prepacked)",
                                name(&pre),
                                list(&sources),
                                name(&vin.output)
                            ),
                        ));
                    }
                    for p in &sources {
                        unused.push((
                            p.clone(),
                            format!(
                                "the pre-packed {} is used (packing.prefer = prepacked)",
                                name(&pre)
                            ),
                        ));
                    }
                    continue;
                }
                if mismatch.is_some() {
                    // The recipe fails below; the pre-packed job stays: no `retain`, no `PACK-CONFLICT` skip.
                    kept = Some(name(&pre));
                } else {
                    report.reason =
                        Some(format!("{} is replaced (packing.prefer = raw)", name(&pre)));
                    jobs.retain(|j| j.input != pre);
                    skipped.push(SkippedFile {
                        path: pre.clone(),
                        output: pre_output,
                        set: Some(set.key.clone()),
                        code: Some(Code::PackConflict),
                        reason: format!(
                            "replaced by {} packed from {} (packing.prefer = raw)",
                            name(&vin.output),
                            list(&sources)
                        ),
                    });
                }
            }
            consumed.extend(sources.iter().cloned());
            if let Some(found) = mismatch {
                report.status = RecipeStatus::Failed;
                report.reason = Some(match kept {
                    Some(pre) => {
                        format!("sources differ in size; {pre} is kept (packing.prefer = raw)")
                    }
                    None => "sources differ in size".into(),
                });
                reports.push(report);
                jobs.push(size_mismatch_job(ctx, &vin, set, recipe.kind, &found));
                continue;
            }
            reports.push(report);
            let job = PackJob {
                kind: recipe.kind,
                channels,
                gray,
                sources: recipe
                    .sources()
                    .into_iter()
                    .map(|(path, role)| PackSource {
                        stamp: SourceStamp::of(&path),
                        path,
                        role,
                    })
                    .collect(),
            };
            out.candidates.push(Candidate {
                set_key: set.key.clone(),
                kind: recipe.kind,
                virtual_input: vin,
                job,
                defaulted,
            });
        }
        for (p, reason) in unused {
            if !failed.contains(&p) {
                retag(
                    jobs,
                    &p,
                    Code::PackUnusedInput,
                    format!("{}: {reason}; not converted", name(&p)),
                );
            }
        }
        out.reports.insert(set.key.clone(), reports);
    }
    // DECISIONS Note M8: the recipe job stands for the raw jobs it consumed.
    jobs.retain(|j| {
        !(consumed.contains(&j.input) && matches!(j.status, JobStatus::PackingInput { .. }))
    });
    out
}

/// Pass 2 for one candidate: resolve the assembled image as the packed sub-type (provenance `recipe:<kind>`),
/// then record the channel provenance (`pack.r` …), one `PACK-DEFAULTED` per defaulted channel and one
/// `CHAN-NOT-GRAY` per non-grey source. The caller adds `provenance["target"]`, as for every job. Everything about
/// the packed channels uses `c.job.gray`, never the final profile's `channels.grayscale_source` (Note M6).
pub fn finish_recipe(
    ctx: &ResolveCtx,
    c: &Candidate,
    set: &TextureSet,
    analysis: &Analysis,
    info: &SourceInfo,
    non_gray: &[PathBuf],
) -> ResolvedJob {
    let facts = FileFacts {
        input: &c.virtual_input,
        suffix: Some(c.kind.suffix()),
        alias: None,
        set,
        analysis,
        info,
    };
    let decision = SubtypeDecision {
        subtype: c.kind.subtype(),
        provenance: Provenance::Recipe(c.kind.id().into()),
        decided: true,
        suggestions: vec![],
    };
    let mut job = resolve_decided(ctx, &facts, decision);
    for (ch, src) in ["r", "g", "b", "a"].iter().zip(&c.job.channels) {
        if let Some(p) = src.provenance(c.job.gray) {
            job.provenance.insert(format!("pack.{ch}"), p);
        }
    }
    let out = name(&c.virtual_input.output);
    for &(i, slot) in &c.defaulted {
        if let ChannelSource::Default { value } = &c.job.channels[i] {
            job.warnings.push(
                Warning::new(
                    Code::PackDefaulted,
                    format!(
                        "{out} {}: no {} source; neutral default {value}",
                        CHANNELS[i],
                        slot.id()
                    ),
                )
                .with_data(json!({
                    "output": slash(&c.virtual_input.output),
                    "channel": CHANNELS[i],
                    "slot": slot,
                    "value": value,
                })),
            );
        }
    }
    for p in non_gray {
        job.warnings.push(Warning::new(
            Code::ChanNotGray,
            format!(
                "{}: R/G/B differ by more than 4/255; packing its {}",
                name(p),
                match c.job.gray {
                    GraySource::R => "R",
                    GraySource::Luma => "luma",
                }
            ),
        ));
    }
    job.pack = Some(c.job.clone());
    job
}
