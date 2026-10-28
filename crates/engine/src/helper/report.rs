//! Note DX: one report per helper of a planned batch: what it is, where it really went, what else it could fill and,
//! when it fills nothing, why. Built from the finished plan, so the GUI never decides any of it.
use super::plan::{accepts, plain, FillPlan};
use super::{AlphaTarget, FillTarget, HelperRead, HelperRole};
use crate::codes::Code;
use crate::config::{RawMap, Target};
use crate::discover::{InputFile, InputKind};
use crate::group::TextureSet;
use crate::pack::plan::raw_role;
use crate::pack::{ChannelSource, RecipeKind};
use crate::policy::{helper_requested, JobStatus, ResolveCtx, ResolvedJob};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct HelperReport {
    pub path: PathBuf,
    pub set_key: String,
    pub role: HelperRole,
    pub role_hand_set: bool,
    /// Where it really went: packed channels, an alpha fill, the base normals a layer blends into.
    pub fills: Vec<FillTarget>,
    pub fills_hand_set: bool,
    /// The alpha targets planned in its set (Ruling 4) and the channels of the packed recipes valid there.
    pub options: Vec<FillTarget>,
    pub skipped_targets: Vec<(FillTarget, String)>,
    /// Why it fills nothing (`None` when `fills` is not empty).
    pub not_used: Option<String>,
    pub read: Option<HelperRead>,
    pub invert: bool,
    /// A normal-type role's `rnm.target` in effect: hand-set, else its name's prefix, else `_n` falling back to `_cnr`
    /// (Note W4). `None` for other roles.
    pub rnm_target: Option<crate::config::RnmTarget>,
    /// The outputs it fills.
    pub outputs: Vec<PathBuf>,
}

/// The texts that say why a helper's own job converts nothing.
const NOT_USED: [Code; 6] = [
    Code::PackUnusedInput,
    Code::InPackingInput,
    Code::RnmNoBase,
    Code::RnmMaskOrphan,
    Code::RnmIgnoredEye,
    Code::RnmLayerShadowed,
];

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The channels a packed recipe holds a source in (`_p` and the env-mask `_m` have only R).
fn channels(kind: RecipeKind) -> impl Iterator<Item = u8> {
    (0..4u8).filter(move |&c| crate::pack::slot_of(kind, usize::from(c)).is_some())
}

/// The cs or vanilla recipes a hand-set fill may name in this set (`packed_target`'s rules), whether planned or not.
fn packed_kinds(ctx: &ResolveCtx, set: &str, planned: &[RecipeKind]) -> Vec<RecipeKind> {
    match ctx.target {
        Target::Cs => {
            let cs = planned
                .iter()
                .copied()
                .find(|k| matches!(k, RecipeKind::Rmaos | RecipeKind::Rfaos))
                .unwrap_or(RecipeKind::Rmaos);
            vec![cs, RecipeKind::Parallax]
        }
        Target::Vanilla => {
            let complex = ctx
                .shaders
                .get(set)
                .is_some_and(|p| p.shader == crate::shader::Shader::ComplexMaterial);
            vec![if complex {
                RecipeKind::CpmM
            } else {
                RecipeKind::VanillaM
            }]
        }
    }
}

/// Every helper of every set, by set key then file name. Call after `plan_rnm`, so layers have their bases.
pub fn reports(
    ctx: &ResolveCtx,
    sets: &[TextureSet],
    files: &[InputFile],
    jobs: &[ResolvedJob],
    fills: &FillPlan,
) -> Vec<HelperReport> {
    let mut out = vec![];
    let mut sets: Vec<&TextureSet> = sets.iter().collect();
    sets.sort_by(|a, b| a.key.cmp(&b.key));
    for set in sets {
        let in_set: Vec<&ResolvedJob> = jobs.iter().filter(|j| j.set_key == set.key).collect();
        let mut members: Vec<&InputFile> = set
            .members
            .iter()
            .map(|&i| &files[i])
            .filter(|f| f.kind == InputKind::Image)
            .collect();
        members.sort_by_key(|f| file_name(&f.path));
        for f in members {
            let sel = helper_requested(ctx, f);
            // A recipe's job has its first source as `input` (Note M8); it is not that source's own job.
            let own = jobs
                .iter()
                .find(|j| j.input == f.path && j.pack.is_none() && j.derive.is_none());
            let packed_role = in_set.iter().find_map(|j| {
                let pack = j
                    .pack
                    .iter()
                    .flat_map(|p| &p.sources)
                    .map(|s| (&s.path, s.role));
                let fill = j.fill.iter().map(|x| (&x.source.path, x.source.role));
                pack.chain(fill)
                    .find(|(p, r)| **p == f.path && *r != RawMap::Rmaos)
                    .map(|(_, r)| r)
            });
            let role = match (sel.role, own.map(|j| &j.status)) {
                (Some(HelperRole::NotHelper), _) => continue,
                (Some(r), _) => r,
                (None, Some(JobStatus::PackingInput { role })) => HelperRole::Raw(*role),
                (None, Some(JobStatus::NormalLayer { term })) => {
                    HelperRole::Layer(crate::rnm::LayerTermId {
                        prefix: None,
                        ..*term
                    })
                }
                // Converted as an ordinary file (a sub-type was named): no helper.
                (None, Some(_)) => continue,
                // Consumed by a recipe or a fill, or failed or cancelled before it had a job.
                (None, None) => match packed_role.or_else(|| raw_role(f)) {
                    Some(r) => HelperRole::Raw(r),
                    None => continue,
                },
            };
            let mut went: Vec<(FillTarget, PathBuf)> = vec![];
            // A layer attached to a base whose RNM is switched off: the reason it fills nothing.
            let mut rnm_off: Option<String> = None;
            for j in &in_set {
                if let Some(pk) = &j.pack {
                    for (c, ch) in pk.channels.iter().enumerate() {
                        if matches!(ch, ChannelSource::Source { path, .. } if *path == f.path) {
                            let t = FillTarget::Packed {
                                suffix: pk.kind.suffix(),
                                channel: c as u8,
                            };
                            went.push((t, j.output.clone()));
                        }
                    }
                }
                if j.fill.as_ref().is_some_and(|x| x.source.path == f.path) {
                    if let Some(a) = AlphaTarget::ALL
                        .into_iter()
                        .find(|&a| accepts(a, j.profile.subtype))
                    {
                        went.push((FillTarget::Alpha(a), j.output.clone()));
                    }
                }
                let layer = j.rnm.as_ref().is_some_and(|r| {
                    r.layers.iter().any(|l| {
                        l.file.path == f.path || l.mask.as_ref().is_some_and(|m| m.path == f.path)
                    })
                });
                if let (true, Some(base)) = (layer, j.profile.subtype.rnm_base()) {
                    if j.active_rnm().is_some() {
                        went.push((FillTarget::Rnm(base), j.output.clone()));
                    } else if !j.profile.rnm.enabled {
                        rnm_off.get_or_insert_with(|| {
                            format!(
                                "RNM is off for {} (rnm.enabled = false)",
                                file_name(&j.output)
                            )
                        });
                    }
                }
            }
            let options: Vec<FillTarget> = if role.is_normal() {
                vec![]
            } else {
                let alpha = AlphaTarget::ALL.into_iter().filter(|&a| {
                    in_set
                        .iter()
                        .any(|j| j.input != f.path && plain(j) && accepts(a, j.profile.subtype))
                });
                let planned: Vec<RecipeKind> = in_set
                    .iter()
                    .filter_map(|j| j.pack.as_ref().map(|p| p.kind))
                    .collect();
                let packed = packed_kinds(ctx, &set.key, &planned)
                    .into_iter()
                    .flat_map(|k| {
                        channels(k).map(move |channel| FillTarget::Packed {
                            suffix: k.suffix(),
                            channel,
                        })
                    });
                alpha.map(FillTarget::Alpha).chain(packed).collect()
            };
            let not_used = if !went.is_empty() {
                None
            } else if sel.fills.as_ref().is_some_and(|v| v.is_empty()) {
                Some("no targets chosen".to_string())
            } else {
                // Its own job's reason, else a shadowed layer's note on its base, else RNM off on its base, else the alpha
                // planner's.
                let name = file_name(&f.path);
                own.into_iter()
                    .flat_map(|j| &j.warnings)
                    .find(|w| NOT_USED.contains(&w.code))
                    .or_else(|| {
                        in_set.iter().flat_map(|j| &j.warnings).find(|w| {
                            w.code == Code::RnmLayerShadowed && w.message.starts_with(&name)
                        })
                    })
                    .map(|w| w.message.clone())
                    .or(rnm_off)
                    .or_else(|| fills.not_used.get(&f.path).cloned())
            };
            let rnm_target = role.is_normal().then(|| {
                use crate::config::RnmTarget;
                use crate::rnm::plan::{base_kind, effective_target};
                let prefix = own.and_then(|j| match j.status {
                    JobStatus::NormalLayer { term } => term.prefix,
                    _ => None,
                });
                let has = |k: RnmTarget| in_set.iter().any(|j| base_kind(j) == Some(k));
                effective_target(
                    prefix,
                    crate::policy::rnm_target_requested(ctx, f),
                    has(RnmTarget::N),
                    has(RnmTarget::Cnr),
                )
            });
            let mut outputs: Vec<PathBuf> = vec![];
            for (_, o) in &went {
                if !outputs.contains(o) {
                    outputs.push(o.clone());
                }
            }
            out.push(HelperReport {
                path: f.path.clone(),
                set_key: set.key.clone(),
                role,
                role_hand_set: sel.role.is_some(),
                fills: went.into_iter().map(|(t, _)| t).collect(),
                fills_hand_set: sel.fills.is_some(),
                options,
                skipped_targets: fills
                    .skipped_targets
                    .get(&f.path)
                    .cloned()
                    .unwrap_or_default(),
                not_used,
                read: sel.read,
                invert: sel.invert,
                rnm_target,
                outputs,
            });
        }
    }
    out
}
