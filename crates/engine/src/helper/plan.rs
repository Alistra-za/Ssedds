//! Note DT: which alpha channel each helper of a set fills (Rulings 4-8, 11, 12). Runs before `plan_packing` and
//! decides only from the plain file jobs, the [`AlphaFacts`] and the `helper.*` selectors; packed targets belong to
//! packing (`pack::recipes_for_sel`).
use super::{AlphaTarget, FillJob, FillTarget, HelperRole, HelperSel};
use crate::config::{RawMap, Subtype, Target};
use crate::discover::{InputFile, InputKind};
use crate::group::TextureSet;
use crate::pack::plan::{raw_role, size_mismatch_warning};
use crate::pack::PackSource;
use crate::policy::{helper_requested, JobStatus, ResolveCtx, ResolvedJob};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What pass 1 knows of a file's alpha and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaFacts {
    /// Ruling 6: no alpha, a constant 255 alpha, or alpha min == max, judged on the original source.
    pub empty: bool,
    pub size: (u32, u32),
}

/// A helper landed on a target: `target` is the target job's `input` (jobs are found by input everywhere).
#[derive(Debug, Clone, PartialEq)]
pub struct FillCandidate {
    pub set_key: String,
    pub target: PathBuf,
    pub fill: FillJob,
}

/// The alpha fills of a batch. `used` and `not_used` cover only helpers that had an alpha target: a helper may miss
/// every alpha target and still feed a packed channel, so the caller reconciles `not_used` with packing's unused list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FillPlan {
    pub candidates: Vec<FillCandidate>,
    pub used: BTreeSet<PathBuf>,
    /// Helper → why it fills no alpha (the first reason over its targets).
    pub not_used: BTreeMap<PathBuf, String>,
    /// Helper → hand-set targets its role cannot fill.
    pub skipped_targets: BTreeMap<PathBuf, Vec<(FillTarget, String)>>,
}

/// Ruling 5: the alpha a role fills when nothing is hand-set (`cnr.a` never: base roughness is not coat roughness).
fn suggested(role: RawMap) -> Option<AlphaTarget> {
    match role {
        RawMap::Opacity => Some(AlphaTarget::Diffuse),
        RawMap::Specular => Some(AlphaTarget::N),
        RawMap::FuzzWeight => Some(AlphaTarget::F),
        RawMap::SubsurfaceWeight => Some(AlphaTarget::S),
        _ => None,
    }
}

/// Ruling 4: the sub-types whose alpha `t` is.
pub(super) fn accepts(t: AlphaTarget, s: Subtype) -> bool {
    match t {
        AlphaTarget::Diffuse => matches!(
            s,
            Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::SkinAlbedo
        ),
        AlphaTarget::N => s == Subtype::Normal,
        AlphaTarget::F => s == Subtype::PbrFuzz,
        AlphaTarget::S => s == Subtype::PbrSubsurface,
        AlphaTarget::Cnr => s == Subtype::CoatNormal,
    }
}

fn what(t: AlphaTarget) -> &'static str {
    match t {
        AlphaTarget::Diffuse => "colour texture",
        AlphaTarget::N => "non-PBR _n",
        AlphaTarget::F => "_f",
        AlphaTarget::S => "_s",
        AlphaTarget::Cnr => "_cnr",
    }
}

fn name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Ruling 4: a plain file job (no recipe, derive or cube) that converts.
pub(super) fn plain(j: &ResolvedJob) -> bool {
    j.pack.is_none()
        && j.derive.is_none()
        && j.cube.is_none()
        && !matches!(
            j.status,
            JobStatus::Error { .. }
                | JobStatus::PackingInput { .. }
                | JobStatus::NormalLayer { .. }
        )
}

struct Helper<'a> {
    f: &'a InputFile,
    role: RawMap,
    sel: HelperSel,
    targets: Vec<AlphaTarget>,
}

/// Plans every set's alpha fills (Rulings 4-8). An assigned fill whose size differs from its target's turns the
/// target job into a `PACK-SIZE-MISMATCH` error; nothing else changes `jobs`.
pub fn plan_fills(
    ctx: &ResolveCtx,
    sets: &[TextureSet],
    files: &[InputFile],
    jobs: &mut [ResolvedJob],
    facts: &BTreeMap<PathBuf, AlphaFacts>,
) -> FillPlan {
    let mut out = FillPlan::default();
    for set in sets {
        let members: Vec<&InputFile> = set.members.iter().map(|&i| &files[i]).collect();
        let mut helpers = vec![];
        for &f in &members {
            // Ruling 12: images only.
            if f.kind != InputKind::Image {
                continue;
            }
            let sel = helper_requested(ctx, f);
            let role = match sel.role {
                Some(HelperRole::NotHelper) => continue,
                Some(HelperRole::Layer(_)) => {
                    // Packing never sees a layer role, so every hand-set target is reported here.
                    if let Some(fills) = sel.fills.as_ref().filter(|v| !v.is_empty()) {
                        out.skipped_targets.insert(
                            f.path.clone(),
                            fills
                                .iter()
                                .map(|&t| (t, "normal-type roles feed RNM layers only".into()))
                                .collect(),
                        );
                    }
                    continue;
                }
                Some(HelperRole::Raw(r)) => r,
                // Note DR: a skin `_f` has its role on its job only.
                None => match raw_role(f).or_else(|| {
                    jobs.iter().find_map(|j| match j.status {
                        JobStatus::PackingInput { role } if j.input == f.path => Some(role),
                        _ => None,
                    })
                }) {
                    Some(r) => r,
                    None => continue,
                },
            };
            let targets: Vec<AlphaTarget> = match &sel.fills {
                Some(v) => v
                    .iter()
                    .filter_map(|t| match t {
                        FillTarget::Alpha(a) => Some(*a),
                        FillTarget::Packed { .. } | FillTarget::Rnm(_) => None,
                    })
                    .collect(),
                None => suggested(role).into_iter().collect(),
            };
            if !targets.is_empty() {
                helpers.push(Helper {
                    f,
                    role,
                    sel,
                    targets,
                });
            }
        }
        // Ruling 8: assigned before suggested, then the first by path.
        helpers.sort_by(|a, b| (!a.sel.assigned(), &a.f.path).cmp(&(!b.sel.assigned(), &b.f.path)));
        let mut winner: BTreeMap<String, PathBuf> = BTreeMap::new();
        for h in helpers {
            let assigned = h.sel.assigned();
            let mut landed = false;
            let mut why = vec![];
            for t in h.targets {
                let id = FillTarget::Alpha(t).id();
                if let Some(w) = winner.get(&id) {
                    why.push(format!("{id} is filled by {}", name(w)));
                    continue;
                }
                // Two matching jobs in one set: the first by input.
                let Some(ji) = (0..jobs.len())
                    .filter(|&i| {
                        let j = &jobs[i];
                        j.input != h.f.path
                            && members.iter().any(|m| m.path == j.input)
                            && plain(j)
                            && accepts(t, j.profile.subtype)
                    })
                    .min_by(|&a, &b| jobs[a].input.cmp(&jobs[b].input))
                else {
                    let under = if ctx.target == Target::Vanilla {
                        " under vanilla"
                    } else {
                        ""
                    };
                    why.push(format!("no {} planned{under}", what(t)));
                    continue;
                };
                let target = jobs[ji].input.clone();
                // A file without facts failed to load; its own error reports it.
                let (Some(hf), Some(tf)) = (facts.get(&h.f.path), facts.get(&target)) else {
                    let missing = if facts.contains_key(&h.f.path) {
                        &target
                    } else {
                        &h.f.path
                    };
                    why.push(format!("{} was not analysed", name(missing)));
                    // An assigned fill still holds its target: no suggested helper takes it instead.
                    if assigned {
                        winner.insert(id, h.f.path.clone());
                    }
                    continue;
                };
                let fill = FillJob {
                    source: PackSource {
                        // From the helper's planned job, never the file system (a pure planner).
                        stamp: jobs
                            .iter()
                            .find(|j| j.input == h.f.path)
                            .and_then(|j| j.source),
                        path: h.f.path.clone(),
                        role: h.role,
                    },
                    read: h.sel.read,
                    invert: h.sel.invert,
                    assigned,
                };
                if hf.size != tf.size {
                    let ((hw, hh), (tw, th)) = (hf.size, tf.size);
                    if !assigned {
                        // Ruling 7: a suggestion never makes an existing project fail.
                        why.push(format!("{hw}x{hh}, target {tw}x{th}: sizes differ"));
                        continue;
                    }
                    // Ruling 7: the user asked; never silently drop it.
                    let j = &mut jobs[ji];
                    j.status = JobStatus::Error {
                        codes: vec![crate::codes::Code::PackSizeMismatch],
                    };
                    let w = size_mismatch_warning(
                        &j.output,
                        &[(h.f.path.clone(), hw, hh), (target.clone(), tw, th)],
                    );
                    j.warnings.push(w);
                    // The error job keeps the helper, so the run ends the helper with this output (never run).
                    j.fill = Some(fill);
                    winner.insert(id, h.f.path.clone());
                    landed = true;
                    continue;
                }
                if !assigned && !tf.empty {
                    why.push(format!("{} already has alpha", name(&target)));
                    continue;
                }
                out.candidates.push(FillCandidate {
                    set_key: set.key.clone(),
                    target,
                    fill,
                });
                winner.insert(id, h.f.path.clone());
                landed = true;
            }
            if landed {
                out.used.insert(h.f.path.clone());
            } else if let Some(r) = why.into_iter().next() {
                out.not_used.insert(h.f.path.clone(), r);
            }
        }
    }
    out
}
