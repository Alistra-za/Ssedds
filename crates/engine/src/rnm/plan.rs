//! M6 Track F2 (spec 2.3, 3.3; DECISIONS Note W4, W5, W9): attach each set's layer files to its base normals.
use super::{LayerTerm, RnmAo, RnmFile, RnmJob, RnmLayer};
use crate::codes::{Code, Warning};
use crate::config::{BentMode, LayerId, RnmTarget};
use crate::discover::InputFile;
use crate::group::{split_suffix_with, vocab_of, TextureSet, VocabRole};
use crate::policy::{rnm_target_requested, JobStatus, ResolveCtx, ResolvedJob, SourceStamp};
use std::collections::BTreeMap;
use std::path::Path;

fn file(p: &Path) -> RnmFile {
    RnmFile {
        path: p.to_path_buf(),
        stamp: SourceStamp::of(p),
    }
}

fn name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// A base job of the set: planned, not packed, a sub-type with an `rnm_base`.
pub(crate) fn base_kind(j: &ResolvedJob) -> Option<RnmTarget> {
    if j.pack.is_some()
        || matches!(
            j.status,
            JobStatus::Error { .. }
                | JobStatus::NormalLayer { .. }
                | JobStatus::PackingInput { .. }
        )
    {
        return None;
    }
    j.profile.subtype.rnm_base()
}

/// The bases a layer feeds: `requested`, else its name's prefix, else `n` falling back to `cnr` (Note W4).
fn targets(
    term: LayerTerm,
    requested: Option<RnmTarget>,
    has_n: bool,
    has_cnr: bool,
) -> Vec<RnmTarget> {
    match effective_target(term.prefix, requested, has_n, has_cnr) {
        RnmTarget::Both => vec![RnmTarget::N, RnmTarget::Cnr],
        one => vec![one],
    }
}

/// The `rnm.target` a layer has in effect: `requested`, else its name's `prefix`, else `n` falling back to `cnr`.
pub(crate) fn effective_target(
    prefix: Option<RnmTarget>,
    requested: Option<RnmTarget>,
    has_n: bool,
    has_cnr: bool,
) -> RnmTarget {
    requested.or(prefix).unwrap_or(if !has_n && has_cnr {
        RnmTarget::Cnr
    } else {
        RnmTarget::N
    })
}

/// Rank of a candidate for one layer in a base `kind`'s stack, lowest first (Note W4).
fn rank(term: LayerTerm, kind: RnmTarget) -> u8 {
    match term.prefix {
        Some(p) if p == kind => 0,
        None => 1,
        Some(_) => 2,
    }
}

pub fn plan_rnm(
    ctx: &ResolveCtx,
    sets: &[TextureSet],
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
    jobs: &mut [ResolvedJob],
) {
    // One pass: job indices per set, so each set scans only its own jobs.
    let mut by_set: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, j) in jobs.iter().enumerate() {
        by_set.entry(j.set_key.clone()).or_default().push(i);
    }
    let by_path: BTreeMap<&Path, &InputFile> =
        files.iter().map(|f| (f.path.as_path(), f)).collect();
    for set in sets {
        let idx: Vec<usize> = by_set.remove(&set.key).unwrap_or_default();
        // (job index, term) of the set's layer files.
        let layers: Vec<(usize, LayerTerm)> = idx
            .iter()
            .map(|&i| (i, &jobs[i]))
            .filter_map(|(i, j)| match j.status {
                JobStatus::NormalLayer { term } => Some((i, term.into())),
                _ => None,
            })
            .collect();
        if layers.is_empty() {
            continue;
        }
        let eye = idx.iter().any(|&i| {
            jobs[i]
                .shader
                .as_ref()
                .is_some_and(|p| p.shader == crate::shader::Shader::Eye)
        });
        if eye {
            for &(i, _) in &layers {
                let msg = format!(
                    "{}: eye normals are model-space, so RNM is not applied (F2 spec 2.3)",
                    name(&jobs[i].input)
                );
                jobs[i]
                    .warnings
                    .push(Warning::new(Code::RnmIgnoredEye, msg));
            }
            continue;
        }
        let bases: Vec<(usize, RnmTarget)> = idx
            .iter()
            .filter_map(|&i| base_kind(&jobs[i]).map(|k| (i, k)))
            .collect();
        let has = |k: RnmTarget| bases.iter().any(|(_, b)| *b == k);
        // Read before any warning is pushed, so no borrow of `jobs` is held across the mutations.
        let reqs: Vec<Option<RnmTarget>> = layers
            .iter()
            .map(|&(i, _)| {
                by_path
                    .get(jobs[i].input.as_path())
                    .and_then(|f| rnm_target_requested(ctx, f))
            })
            .collect();
        // Layer files (not masks) with the bases they feed.
        let mut feeds: Vec<(usize, LayerTerm, Vec<RnmTarget>)> = vec![];
        for (&(i, term), &req) in layers.iter().zip(&reqs).filter(|((_, t), _)| !t.mask) {
            let ts: Vec<RnmTarget> = targets(term, req, has(RnmTarget::N), has(RnmTarget::Cnr))
                .into_iter()
                .filter(|t| has(*t))
                .collect();
            if ts.is_empty() {
                let msg = format!(
                    "{}: no base normal it targets (_n or _cnr) in its set, so it is not used",
                    name(&jobs[i].input)
                );
                jobs[i].warnings.push(Warning::new(Code::RnmNoBase, msg));
            }
            feeds.push((i, term, ts));
        }
        // Masks pair with the layer of the same id and prefix.
        let mask_of = |term: LayerTerm| {
            layers
                .iter()
                .find(|(_, t)| t.mask && t.layer == term.layer && t.prefix == term.prefix)
                .map(|(i, _)| *i)
        };
        for &(i, term) in layers.iter().filter(|(_, t)| t.mask) {
            if !feeds
                .iter()
                .any(|(_, t, _)| t.layer == term.layer && t.prefix == term.prefix)
            {
                let msg = format!(
                    "{}: a mask without its layer file, ignored",
                    name(&jobs[i].input)
                );
                jobs[i]
                    .warnings
                    .push(Warning::new(Code::RnmMaskOrphan, msg));
            }
        }
        let ao = ao_source(set, files, aliases);
        for &(b, kind) in &bases {
            let mut stack: Vec<RnmLayer> = vec![];
            let mut notes: Vec<Warning> = vec![];
            for id in LayerId::ORDER {
                let mut cands: Vec<&(usize, LayerTerm, Vec<RnmTarget>)> = feeds
                    .iter()
                    .filter(|(_, t, ts)| t.layer == id && ts.contains(&kind))
                    .collect();
                cands.sort_by(|a, c| {
                    rank(a.1, kind)
                        .cmp(&rank(c.1, kind))
                        .then_with(|| jobs[a.0].input.cmp(&jobs[c.0].input))
                });
                let Some(&&(win, term, _)) = cands.first() else {
                    continue;
                };
                for &&(lose, _, _) in &cands[1..] {
                    notes.push(Warning::new(
                        Code::RnmLayerShadowed,
                        format!(
                            "{} is not used for this normal's {} layer: {} is",
                            name(&jobs[lose].input),
                            id.label(),
                            name(&jobs[win].input)
                        ),
                    ));
                }
                stack.push(RnmLayer {
                    id,
                    file: file(&jobs[win].input),
                    mask: mask_of(term).map(|m| file(&jobs[m].input)),
                });
            }
            if stack.is_empty() {
                continue;
            }
            let job = &mut jobs[b];
            let wants_ao = stack.iter().any(|l| l.id == LayerId::Bent)
                && job.profile.rnm.bent.mode == BentMode::AoBlend
                && job.profile.rnm.bent.ao;
            if wants_ao && ao.is_none() {
                notes.push(Warning::new(
                    Code::RnmAoMissing,
                    "the bent layer has no AO (no _ao, _orm or _rmaos/_rfaos file), so its weight is uniform",
                ));
            }
            job.warnings.extend(notes);
            job.rnm = Some(RnmJob {
                layers: stack,
                ao: ao.clone(),
            });
        }
    }
}

/// Note W9: the set's raw AO map (R), else its `_orm` (R, Note AL4), else its `_rmaos`/`_rfaos` file (B).
fn ao_source(
    set: &TextureSet,
    files: &[InputFile],
    aliases: &BTreeMap<String, String>,
) -> Option<RnmAo> {
    let stem = |f: &InputFile| {
        f.rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let members = || set.members.iter().map(|&i| &files[i]);
    let role_is = |f: &InputFile, role| {
        vocab_of(&stem(f)).map(|v| v.role) == Some(VocabRole::PackingInput(role))
    };
    // Note AL4: a loose `_ao`, else an `_orm` (AO in R).
    let raw = members()
        .find(|f| role_is(f, crate::config::RawMap::Ao))
        .or_else(|| members().find(|f| role_is(f, crate::config::RawMap::Orm)));
    if let Some(f) = raw {
        return Some(RnmAo {
            file: file(&f.path),
            channel: 0,
        });
    }
    members()
        .find(|f| {
            f.kind == crate::discover::InputKind::Image
                && matches!(
                    split_suffix_with(&stem(f), aliases).suffix.as_deref(),
                    Some("rmaos" | "rfaos")
                )
        })
        .map(|f| RnmAo {
            file: file(&f.path),
            channel: 2,
        })
}
