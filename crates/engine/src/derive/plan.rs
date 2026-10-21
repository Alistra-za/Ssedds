//! The derive step of `Engine::plan` (DECISIONS Note CM): with `BatchRequest::derive`, each PBR set's jobs are
//! replaced by derived jobs. [`plan_derived`] runs after packing pass 2 and returns the candidates; the caller
//! assembles, analyses and resolves each with [`finish_derived`] under pass 2's admission.
use super::recipe::nif_glossiness;
use super::{
    vanilla_game_path, vanilla_rel, DeriveJob, DeriveKind, DeriveMode, DeriveRequest, DeriveSet,
    DeriveSource,
};
use crate::analyse::Analysis;
use crate::api::{FailedFile, SkippedFile};
use crate::codes::{Code, Warning};
use crate::config::AlphaPolicy;
use crate::discover::InputFile;
use crate::group::TextureSet;
use crate::image::SourceInfo;
use crate::pack::assemble::{check_sizes, PackError};
use crate::pack::plan::{recipe_error_job, size_mismatch_warning, virtual_input_as};
use crate::pack::ChannelSource;
use crate::policy::{
    resolve_decided, FileFacts, JobStatus, ResolveCtx, ResolvedJob, SourceStamp, SubtypeDecision,
};
use crate::presets::Provenance;
use crate::shader::{mesh_key, Shader, ShaderOrigin, ShaderPick};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One derived output that passed planning: assembled, analysed and resolved by the caller, as a packing
/// `Candidate` is.
pub struct DeriveCandidate {
    pub set_key: String,
    /// The file it resolves with (TA15): the albedo's for a diffuse, the normal's for the `_n`, the virtual
    /// `<base>_m` beside the colour member for an `_m`; output and game path folded (TA12), `rel` kept for `[files]`.
    pub input: InputFile,
    pub job: DeriveJob,
    /// Set-level notes this job carries (it is the set's diffuse, else its first derived job).
    pub notes: Vec<Warning>,
    /// `Some(env_map)`: this job carries `DERIVE-NIF-SUGGEST`, its glossiness from the mean roughness.
    pub nif: Option<bool>,
}

#[derive(Default)]
pub struct DerivePlan {
    pub candidates: Vec<DeriveCandidate>,
    /// One pick per derived set key (TA29), replacing the fallback pick.
    pub picks: BTreeMap<String, ShaderPick>,
    /// The vanilla set key of each removed member's job or skip, for [`report_unread`]'s skips.
    pub member_sets: BTreeMap<PathBuf, String>,
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn source_path(c: &ChannelSource) -> Option<&Path> {
    match c {
        ChannelSource::Source { path, .. } => Some(path),
        _ => None,
    }
}

/// Every file a set's slots read.
fn reads(set: &DeriveSet) -> impl Iterator<Item = &Path> {
    [&set.rough, &set.metal, &set.ao]
        .into_iter()
        .chain(
            [&set.height, &set.curves, &set.cavity]
                .into_iter()
                .flatten(),
        )
        .filter_map(source_path)
        .chain(set.albedo.as_deref())
        .chain(set.normal.as_deref())
}

/// The derived job of `kind` for `set`: the slots it reads, and every file read once, in slot order. A kind that
/// reads no colour map (`_m`) is sized by `anchor` (TA22), its first source.
fn derive_job(kind: DeriveKind, set: &DeriveSet, d: &DeriveRequest, anchor: &Path) -> DeriveJob {
    use DeriveKind::*;
    let diffuse = matches!(kind, VanillaDiffuse | CmDiffuse | GrassStack);
    let mut job = DeriveJob {
        kind,
        // Task 5 adds landscape sets.
        landscape: false,
        albedo: set.albedo.clone().filter(|_| diffuse),
        normal: set
            .normal
            .clone()
            .filter(|_| matches!(kind, LegacyN | GrassStack)),
        rough: set.rough.clone(),
        metal: set.metal.clone(),
        ao: set.ao.clone(),
        height: set.height.clone().filter(|_| kind == CmM),
        curves: set.curves.clone().filter(|_| diffuse),
        cavity: set.cavity.clone().filter(|_| diffuse),
        gray: set.gray,
        params: d.params,
        sources: vec![],
    };
    let mut sources: Vec<DeriveSource> = vec![];
    let mut add = |p: &Path, role: &str| match sources.iter_mut().find(|s| s.path == p) {
        Some(s) => s.role = format!("{}+{role}", s.role),
        None => sources.push(DeriveSource {
            path: p.to_path_buf(),
            role: role.into(),
            stamp: SourceStamp::of(p),
        }),
    };
    if let Some(a) = &job.albedo {
        add(a, "albedo");
    }
    if let Some(n) = &job.normal {
        add(n, "normal");
    }
    if job.albedo.is_none() && job.normal.is_none() {
        add(anchor, "size");
    }
    let slots = [
        ("rough", Some(&job.rough)),
        ("metal", Some(&job.metal)),
        ("ao", Some(&job.ao)),
        ("height", job.height.as_ref()),
        ("curves", job.curves.as_ref()),
        ("cavity", job.cavity.as_ref()),
    ];
    for (role, c) in slots {
        if let Some(p) = c.and_then(source_path) {
            add(p, role);
        }
    }
    job.sources = sources;
    job
}

/// Replaces the jobs of every set in `d` (and of `d.drop`) with derived candidates (DECISIONS Note CM). Jobs whose
/// input or packed source is a member are removed, with the members' `PACK-CONFLICT` skips. A set that cannot be
/// derived becomes a `SkippedFile` (`DERIVE-DDS-SOURCE`, `DERIVE-NO-ALBEDO`) or error jobs (`PACK-SIZE-MISMATCH`,
/// `DERIVE-PATH-CONFLICT`), pushed here; the caller adds their target provenance. A set with a member that failed
/// to load (in `failed`) is not derived: the failure is already reported.
// shortcut: O6 (the landscape `_p` job stays) is Task 5's, with landscape and grass sets (preflight P7).
pub fn plan_derived(
    ctx: &ResolveCtx,
    d: &DeriveRequest,
    mesh_shaders: &BTreeMap<String, Vec<Shader>>,
    files: &[InputFile],
    jobs: &mut Vec<ResolvedJob>,
    skipped: &mut Vec<SkippedFile>,
    failed: &[FailedFile],
) -> DerivePlan {
    use DeriveKind::*;
    let by_path: BTreeMap<&Path, &InputFile> =
        files.iter().map(|f| (f.path.as_path(), f)).collect();
    // The vanilla plan's set key of each file, from its job or skip, read before the jobs go.
    let member_sets: BTreeMap<PathBuf, String> = jobs
        .iter()
        .map(|j| (&j.input, &j.set_key))
        .chain(
            skipped
                .iter()
                .filter_map(|k| Some((&k.path, k.set.as_ref()?))),
        )
        .map(|(p, k)| (p.clone(), k.clone()))
        .collect();
    let keys: Vec<Option<String>> = d
        .sets
        .iter()
        .map(|s| s.members.iter().find_map(|m| member_sets.get(m).cloned()))
        .collect();
    let gone: BTreeSet<&Path> = d
        .sets
        .iter()
        .flat_map(|s| &s.members)
        .chain(&d.drop)
        .map(PathBuf::as_path)
        .collect();
    jobs.retain(|j| {
        !(gone.contains(j.input.as_path())
            || j.pack
                .as_ref()
                .is_some_and(|p| p.sources.iter().any(|s| gone.contains(s.path.as_path()))))
    });
    skipped.retain(|s| !(s.code == Some(Code::PackConflict) && gone.contains(s.path.as_path())));
    let failed: BTreeSet<&Path> = failed.iter().map(|f| f.path.as_path()).collect();

    // TA12: every output another job writes, so a derived output that lands on one is refused before the
    // IN-DUPLICATE guard sees it. Folded as `pack::plan::other_output` compares paths.
    let fold = |p: &Path| slash(p).to_lowercase();
    let mut taken: BTreeMap<String, PathBuf> = jobs
        .iter()
        .filter(|j| {
            !matches!(
                j.status,
                JobStatus::PackingInput { .. }
                    | JobStatus::NormalLayer { .. }
                    | JobStatus::Error { .. }
            )
        })
        .map(|j| (fold(&j.output), j.input.clone()))
        .collect();

    let mut out = DerivePlan {
        member_sets: member_sets
            .into_iter()
            .filter(|(p, _)| gone.contains(p.as_path()))
            .collect(),
        ..DerivePlan::default()
    };
    for (set, key) in d.sets.iter().zip(keys) {
        if let Some(first) = set.dds.first() {
            // One skip per file (review M2): this replaces its `IN-DDS-SKIPPED`.
            skipped.retain(|s| s.path != *first);
            skipped.push(SkippedFile {
                path: first.clone(),
                output: by_path
                    .get(first.as_path())
                    .map_or_else(|| first.clone(), |f| f.output.clone()),
                set: key,
                code: Some(Code::DeriveDdsSource),
                reason: format!(
                    "set {} is not converted: {} is DDS, and DDS is never read for conversion",
                    set.id,
                    set.dds
                        .iter()
                        .map(|p| name(p))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
            continue;
        }
        let Some(key) = key else { continue };
        let colour = |p: &Option<PathBuf>| p.as_deref().and_then(|p| by_path.get(p).copied());
        let (albedo, normal) = (colour(&set.albedo), colour(&set.normal));
        let Some(anchor) = albedo.or(normal) else {
            let first = set.members.first().cloned().unwrap_or_default();
            skipped.push(SkippedFile {
                output: by_path
                    .get(first.as_path())
                    .map_or_else(|| first.clone(), |f| f.output.clone()),
                path: first,
                set: Some(key),
                code: Some(Code::DeriveNoAlbedo),
                reason: format!("set {} is not converted: no albedo and no _n", set.id),
            });
            continue;
        };
        if reads(set).any(|p| failed.contains(p)) {
            continue;
        }

        let metal_source = source_path(&set.metal).is_some();
        // TA8: otherwise the CM diffuse equals the vanilla one.
        let cm_diffuse =
            d.params.keep_metal_brightness && metal_source && d.params.keep_metal_strength > 0.0;
        let mut kinds = vec![];
        let mut notes = vec![];
        match d.mode {
            DeriveMode::Vanilla => {
                if albedo.is_some() {
                    kinds.push(VanillaDiffuse);
                }
                if normal.is_some() {
                    kinds.push(LegacyN);
                }
                if metal_source {
                    // TA13: the vote at the vanilla mesh key of the colour member.
                    let stem = anchor
                        .output
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let stem = crate::group::split_suffix(&stem).0;
                    let mesh = vanilla_game_path(&mesh_key(&anchor.game_path, &stem));
                    if !d.scanned {
                        notes.push(Warning::new(
                            Code::DeriveNoScan,
                            format!(
                                "set {}: metal found, but no env _m is written: no mesh scan",
                                set.id
                            ),
                        ));
                    } else if mesh_shaders
                        .get(&mesh)
                        .is_some_and(|v| v.contains(&Shader::Envmap))
                    {
                        kinds.push(EnvM);
                    } else {
                        notes.push(
                            Warning::new(
                                Code::DeriveNoEnvmap,
                                format!(
                                    "set {}: metal found, but no scanned mesh uses {mesh}.dds with the Environment Map shader; no env _m is written",
                                    set.id
                                ),
                            )
                            .with_data(json!({ "mesh_key": mesh })),
                        );
                    }
                }
            }
            DeriveMode::Cm => {
                kinds.push(CmM);
                if albedo.is_some() && cm_diffuse {
                    kinds.push(CmDiffuse);
                }
            }
        }
        let shader = match d.mode {
            DeriveMode::Cm => Shader::ComplexMaterial,
            DeriveMode::Vanilla if kinds.contains(&EnvM) => Shader::Envmap,
            DeriveMode::Vanilla => Shader::Default,
        };
        // In CM mode only a CM diffuse would need the albedo (review M1).
        if albedo.is_none() && (d.mode == DeriveMode::Vanilla || cm_diffuse) {
            notes.push(Warning::new(
                Code::DeriveNoAlbedo,
                format!("set {}: no albedo, so no diffuse is written", set.id),
            ));
        }
        // TA11, TA28: members that feed no slot are reported once, on the Vanilla plan.
        if d.mode == DeriveMode::Vanilla {
            let used: BTreeSet<&Path> = reads(set).collect();
            for m in set.members.iter().filter(|m| !used.contains(m.as_path())) {
                let stem = m.file_stem().unwrap_or_default().to_string_lossy();
                let reason = if crate::rnm::split_layer(&stem).is_some() {
                    "detail layer not in the vanilla _n"
                } else {
                    "no vanilla or complex-material map uses it"
                };
                notes.push(
                    Warning::new(
                        Code::DeriveNotConverted,
                        format!("{} is not converted: {reason}", name(m)),
                    )
                    .with_data(json!({ "member": slash(m), "reason": reason })),
                );
            }
        }

        let first_cand = out.candidates.len();
        let any = !kinds.is_empty();
        for kind in kinds {
            let job = derive_job(kind, set, d, &anchor.path);
            let mut input = match kind {
                VanillaDiffuse | CmDiffuse | GrassStack => anchor.clone(),
                LegacyN => normal.unwrap_or(anchor).clone(),
                CmM | EnvM => virtual_input_as(anchor, "m"),
            };
            input.output = vanilla_rel(&input.output, &input.game_path);
            input.game_path = vanilla_game_path(&input.game_path);
            input.path = job.sources[0].path.clone();
            let st = kind.subtype(job.landscape);
            let paths: Vec<PathBuf> = job.sources.iter().map(|s| s.path.clone()).collect();
            // P6 (TA22): sources of different sizes fail at plan time, as a packing recipe does.
            if let Err(PackError::SizeMismatch(found)) = check_sizes(&paths) {
                let w = size_mismatch_warning(&input.output, &found);
                jobs.push(recipe_error_job(ctx, &input, &key, st, kind.id(), w));
                continue;
            }
            if let Some(other) = taken.get(&fold(&input.output)) {
                let w = Warning::new(
                    Code::DerivePathConflict,
                    format!(
                        "{} is also written from {}; that file is kept and set {} is not written there",
                        slash(&input.output),
                        slash(other),
                        set.id
                    ),
                )
                .with_data(json!({ "output": slash(&input.output), "other": slash(other) }));
                jobs.push(recipe_error_job(ctx, &input, &key, st, kind.id(), w));
                continue;
            }
            taken.insert(fold(&input.output), input.path.clone());
            out.candidates.push(DeriveCandidate {
                set_key: key.clone(),
                input,
                job,
                notes: vec![],
                nif: None,
            });
        }
        if any {
            out.picks.insert(
                key.clone(),
                ShaderPick {
                    shader,
                    provenance: ShaderOrigin::Derived,
                    wanted: None,
                },
            );
        }
        let mine = &mut out.candidates[first_cand..];
        let find = |c: &[DeriveCandidate], k: &[DeriveKind]| {
            c.iter().position(|c| k.contains(&c.job.kind))
        };
        if let Some(i) =
            find(mine, &[VanillaDiffuse, CmDiffuse]).or((!mine.is_empty()).then_some(0))
        {
            mine[i].notes = notes;
        }
        if d.mode == DeriveMode::Vanilla {
            if let Some(i) = find(mine, &[LegacyN]).or(find(mine, &[VanillaDiffuse])) {
                mine[i].nif = Some(metal_source);
            }
        }
    }
    out
}

/// Review I1: a `DERIVE-NOT-CONVERTED` skip for each member of `d`'s sets and of `d.drop` that nothing else ends:
/// no derived job in `jobs` reads it (those get a `packed_source` event in `run`), and it has no skip, no earlier
/// failure (`failed`, before the derive admission) and was not cancelled. So every discovered file gets one
/// terminal event, keyed by its own output: `_g` and RNM layers, members left out (C2), the other members of a set
/// skipped for DDS or no albedo, and the sources of an error or failed derived job.
pub fn report_unread(
    d: &DeriveRequest,
    member_sets: &BTreeMap<PathBuf, String>,
    files: &[InputFile],
    jobs: &[ResolvedJob],
    skipped: &mut Vec<SkippedFile>,
    failed: &[FailedFile],
    cancelled: &[PathBuf],
) {
    let mut ended: BTreeSet<&Path> = jobs
        .iter()
        .filter_map(|j| j.derive.as_ref())
        .flat_map(|dj| dj.sources.iter().map(|s| s.path.as_path()))
        .chain(failed.iter().map(|f| f.path.as_path()))
        .chain(cancelled.iter().map(PathBuf::as_path))
        .collect();
    let had: BTreeSet<PathBuf> = skipped.iter().map(|s| s.path.clone()).collect();
    ended.extend(had.iter().map(PathBuf::as_path));
    let by_path: BTreeMap<&Path, &InputFile> =
        files.iter().map(|f| (f.path.as_path(), f)).collect();
    let members = d
        .sets
        .iter()
        .flat_map(|s| s.members.iter().map(move |m| (m, Some(s))))
        .chain(d.drop.iter().map(|m| (m, None)));
    let mut new = vec![];
    for (m, set) in members {
        // Only discovered files: a hand-built set may name a member outside the inputs.
        let Some(f) = by_path.get(m.as_path()) else {
            continue;
        };
        if !ended.insert(m.as_path()) {
            continue;
        }
        new.push(SkippedFile {
            path: m.clone(),
            output: f.output.clone(),
            set: member_sets.get(m).cloned(),
            code: Some(Code::DeriveNotConverted),
            reason: match set {
                Some(s) => format!("{} is not converted (set {})", name(m), s.id),
                None => format!("{} is not converted: its set is left out", name(m)),
            },
        });
    }
    skipped.extend(new);
}

/// Resolves an assembled candidate as its kind's sub-type (provenance `recipe:<kind>`), with the set's notes and
/// its NIF suggestion. The caller adds `provenance["target"]`, as for every job.
pub fn finish_derived(
    ctx: &ResolveCtx,
    c: &DeriveCandidate,
    set: &TextureSet,
    analysis: &Analysis,
    info: &SourceInfo,
    mean_rough: f64,
) -> ResolvedJob {
    let kind = c.job.kind;
    let facts = FileFacts {
        input: &c.input,
        suffix: kind.suffix(),
        alias: None,
        set,
        analysis,
        info,
    };
    let decision = SubtypeDecision {
        subtype: kind.subtype(c.job.landscape),
        provenance: Provenance::Recipe(kind.id().into()),
        decided: true,
        suggestions: vec![],
    };
    let mut job = resolve_decided(ctx, &facts, decision);
    // CM14: the stack's bottom half is the `_n`, whose specular alpha `remove` would drop (an opaque albedo, mesh
    // evidence or an explicit layer).
    if kind == DeriveKind::GrassStack && job.profile.alpha.policy == AlphaPolicy::Remove {
        job.profile.alpha.policy = AlphaPolicy::Cutout;
        job.provenance
            .insert("alpha.policy".into(), Provenance::Recipe(kind.id().into()));
    }
    job.warnings.extend(c.notes.iter().cloned());
    if let Some(env_map) = c.nif {
        let glossiness = nif_glossiness(mean_rough);
        job.warnings.push(
            Warning::new(
                Code::DeriveNifSuggest,
                format!(
                    "suggested NIF values: glossiness {glossiness:.1}, specular strength 1.0{}",
                    if env_map {
                        ", Environment Map shader"
                    } else {
                        ""
                    }
                ),
            )
            .with_data(json!({
                "glossiness": glossiness,
                "specular_strength": 1.0,
                "env_map": env_map,
            })),
        );
    }
    job.derive = Some(c.job.clone());
    job
}
