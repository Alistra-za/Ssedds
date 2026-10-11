//! Set-level bake decisions (spec §6.2–6.3, DECISIONS R6–R10): which roughness member bakes which `_n`, and the
//! four `SET-*` codes. `Engine::plan` runs this after packing, so a packed `_rmaos`/`_rfaos` is a member like a
//! pre-packed one, and before `set_resolved` and `file_planned`, so both show the decision.
use super::{bake_shift, BakeJob};
use crate::codes::{Code, Warning};
use crate::config::{ColorContent, NormalMips, RawMap, Subtype, Target};
use crate::discover::{InputFile, InputKind, IMAGE_EXTS};
use crate::group::{split_suffix_with, TextureSet};
use crate::policy::{JobStatus, Provenance, ResolvedJob};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What `plan_bakes` reads besides the jobs.
pub struct BakeCtx<'a> {
    pub target: Target,
    pub sets: &'a [TextureSet],
    pub files: &'a [InputFile],
    /// Preset suffix aliases (`PresetRegistry::alias_table`), for reading companions' names.
    pub aliases: &'a BTreeMap<String, String>,
    /// Keys of the sets whose roughness slot is shared (`BatchRequest::shared_roughness`, as set keys; DECISIONS R7).
    pub shared: &'a BTreeSet<String>,
    /// Borrower set key → the key of the set whose `_n` it borrows (`BatchRequest::borrowed_normal`, as set keys;
    /// DECISIONS Note R M4-final-1).
    pub borrowed: &'a BTreeMap<String, String>,
}

fn is_normal_suffix(s: &str) -> bool {
    s == "n"
}

/// Canonical file suffixes (not sub-type ids): `rmaos`, `rfaos` (the `skin_rfaos` output keeps the `_rfaos` suffix),
/// or a raw map that would pack one (roughness, glossiness, metallic, AO).
fn is_roughness_suffix(s: &str) -> bool {
    s == "rmaos" || s == "rfaos" || RawMap::ALL.iter().any(|r| r.starts_rmaos() && r.id() == s)
}

/// The base name and canonical suffix of a file name (aliases, vocabulary and §6.1 suffixes, as Group reads them).
fn base_and_suffix(
    path: &Path,
    aliases: &BTreeMap<String, String>,
) -> Option<(String, Option<String>)> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let s = split_suffix_with(&stem, aliases);
    Some((s.base, s.suffix))
}

/// Ready, or Blocked only by codes the user must acknowledge: the profile is final (DECISIONS R9).
fn decided(j: &ResolvedJob) -> bool {
    matches!(j.status, JobStatus::Ready | JobStatus::Blocked { .. })
}

/// Resolved into something that is (or, once decided, will be) written.
fn writable(j: &ResolvedJob) -> bool {
    !matches!(
        j.status,
        JobStatus::Error { .. } | JobStatus::PackingInput { .. }
    )
}

fn note(j: &mut ResolvedJob, code: Code, msg: String, companion: Option<&Path>) {
    let data = companion.map_or(
        serde_json::Value::Null,
        |p| json!({ "companion": p.to_string_lossy().replace('\\', "/") }),
    );
    j.warnings.push(Warning::new(code, msg).with_data(data));
}

/// The set's member file whose kind is `kind` and whose canonical suffix passes `want`.
fn member_file<'a>(
    cx: &BakeCtx<'a>,
    set: &TextureSet,
    kind: InputKind,
    want: fn(&str) -> bool,
) -> Option<&'a InputFile> {
    set.members.iter().map(|&i| &cx.files[i]).find(|f| {
        f.kind == kind
            && base_and_suffix(&f.rel, cx.aliases)
                .and_then(|(_, s)| s)
                .is_some_and(|s| want(&s))
    })
}

/// A file next to `beside` with the same base name (case-insensitive) whose canonical suffix passes `want`, that the
/// batch does not hold: `(path, is_dds)`, the first by path.
fn sibling_outside(
    beside: &Path,
    want: fn(&str) -> bool,
    batch: &BTreeSet<PathBuf>,
    aliases: &BTreeMap<String, String>,
) -> Option<(PathBuf, bool)> {
    let (base, _) = base_and_suffix(beside, aliases)?;
    let dir = beside.parent()?;
    let mut hits: Vec<(PathBuf, bool)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && !batch.contains(p))
        .filter_map(|p| {
            let ext = p.extension()?.to_string_lossy().to_ascii_lowercase();
            let dds = ext == "dds";
            if !dds && !IMAGE_EXTS.contains(&ext.as_str()) {
                return None;
            }
            let (b, s) = base_and_suffix(&p, aliases)?;
            (b.eq_ignore_ascii_case(&base) && s.as_deref().is_some_and(want)).then_some((p, dds))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// Why a roughness member has no `_n` to bake from (spec §6.3, DECISIONS R8).
fn missing_normal(cx: &BakeCtx, set: &TextureSet, batch: &BTreeSet<PathBuf>, j: &mut ResolvedJob) {
    if let Some(f) = member_file(cx, set, InputKind::Image, is_normal_suffix) {
        let msg = format!(
            "the set's _n ({}) has no pbr_normal job (it failed, was cancelled, is a duplicate or was given another sub-type): the roughness is written without the variance bake",
            f.path.display()
        );
        note(j, Code::SetMissingCompanion, msg, Some(&f.path));
        return;
    }
    if let Some(f) = member_file(cx, set, InputKind::Dds, is_normal_suffix) {
        let msg = format!(
            "the set's _n is only a DDS ({}): DDS inputs are not re-encoded, so the roughness is written without the variance bake",
            f.path.display()
        );
        note(j, Code::SetCompanionDds, msg, Some(&f.path));
        return;
    }
    match sibling_outside(&j.input, is_normal_suffix, batch, cx.aliases) {
        Some((p, true)) => {
            let msg = format!(
                "the set's _n is only a DDS ({}) and is not in this batch: the roughness is written without the variance bake",
                p.display()
            );
            note(j, Code::SetCompanionDds, msg, Some(&p));
        }
        Some((p, false)) => {
            let msg = format!(
                "{} is on disk but not in this batch: the roughness is written without the variance bake; add it to the batch",
                p.display()
            );
            note(j, Code::SetCompanionOutside, msg, Some(&p));
        }
        None => note(
            j,
            Code::SetMissingCompanion,
            "no _n in this PBR set: the roughness is written without the variance bake".into(),
            None,
        ),
    }
}

/// Why a roughness member whose set borrows the `_n` of set `owner` has nothing to bake from (DECISIONS Note R
/// M4-final-1): the owner has no usable `pbr_normal` job.
fn missing_borrowed_normal(cx: &BakeCtx, owner: &TextureSet, j: &mut ResolvedJob) {
    let file = member_file(cx, owner, InputKind::Image, is_normal_suffix)
        .or_else(|| member_file(cx, owner, InputKind::Dds, is_normal_suffix));
    let msg = format!(
        "the set's _n is borrowed from {}, which has no usable pbr_normal job: the roughness is written without the variance bake",
        owner.key.replace('\\', "/")
    );
    note(
        j,
        Code::SetMissingCompanion,
        msg,
        file.map(|f| f.path.as_path()),
    );
}

/// Why a `pbr_normal` has no roughness member to bake into (spec §6.3, DECISIONS R8).
fn missing_roughness(
    cx: &BakeCtx,
    set: &TextureSet,
    batch: &BTreeSet<PathBuf>,
    j: &mut ResolvedJob,
) {
    if let Some(f) = member_file(cx, set, InputKind::Dds, is_roughness_suffix) {
        let msg = format!(
            "the set's roughness member is only a DDS ({}): DDS inputs are not re-encoded, so the normal is renormalised only",
            f.path.display()
        );
        note(j, Code::SetCompanionDds, msg, Some(&f.path));
        return;
    }
    match sibling_outside(&j.input, is_roughness_suffix, batch, cx.aliases) {
        Some((p, true)) => {
            let msg = format!(
                "the set's roughness member is only a DDS ({}) and is not in this batch: the normal is renormalised only",
                p.display()
            );
            note(j, Code::SetCompanionDds, msg, Some(&p));
        }
        Some((p, false)) => {
            let msg = format!(
                "{} is on disk but not in this batch: the normal is renormalised only; add it to bake its variance into the roughness",
                p.display()
            );
            note(j, Code::SetCompanionOutside, msg, Some(&p));
        }
        None => note(
            j,
            Code::SetMissingCompanion,
            "no _rmaos or _rfaos in this PBR set: the normal is renormalised only".into(),
            None,
        ),
    }
}

/// Bake every member from the `_n` at `n`. Returns whether any member bakes.
fn bake_set(jobs: &mut [ResolvedJob], n: usize, members: &[usize]) -> bool {
    if !decided(&jobs[n]) {
        let p = jobs[n].input.clone();
        let msg = format!(
            "the set's _n ({}) is not decided yet: no variance bake until it is",
            p.display()
        );
        for &m in members {
            note(
                &mut jobs[m],
                Code::SetMissingCompanion,
                msg.clone(),
                Some(&p),
            );
        }
        return false;
    }
    // DECISIONS R6: an explicit `mips.normal = renormalize` on the `_n` (any layer but the built-in default) opts out.
    let nj = &jobs[n];
    if nj.profile.mips.normal == NormalMips::Renormalize
        && nj
            .provenance
            .get("mips.normal")
            .is_some_and(|p| *p != Provenance::Builtin)
    {
        return false;
    }
    let Ok(ndims) = ::image::image_dimensions(&nj.input) else {
        return false;
    };
    let (normal, normal_profile, stamp) = (nj.input.clone(), nj.profile.clone(), nj.source);
    let mut any = false;
    for &m in members {
        // DECISIONS R10: `check_profile` already raised TAG-CONTENT-MISMATCH for an sRGB roughness.
        if jobs[m].profile.color.content == ColorContent::Srgb {
            continue;
        }
        let Ok(mdims) = ::image::image_dimensions(&jobs[m].input) else {
            continue;
        };
        match bake_shift(ndims, mdims) {
            Ok(shift) => {
                jobs[m].bake = Some(BakeJob {
                    normal: normal.clone(),
                    normal_profile: normal_profile.clone(),
                    shift,
                    stamp,
                });
                any = true;
            }
            Err(why) => {
                let w = Warning::new(Code::SetBakeRatio, format!("variance bake off: {why}"))
                    .with_data(json!({
                        "normal": [ndims.0, ndims.1],
                        "member": [mdims.0, mdims.1],
                    }));
                jobs[m].warnings.push(w);
            }
        }
    }
    if any && jobs[n].profile.mips.normal != NormalMips::RenormalizeBake {
        jobs[n].profile.mips.normal = NormalMips::RenormalizeBake;
        jobs[n]
            .provenance
            .insert("mips.normal".into(), Provenance::Set);
    }
    any
}

/// The set's `_n`: its first `pbr_normal` job by path that did not fail (DECISIONS R9).
fn own_normal(jobs: &[ResolvedJob], key: &str) -> Option<usize> {
    (0..jobs.len())
        .filter(|&i| {
            jobs[i].set_key == key
                && jobs[i].profile.subtype == Subtype::PbrNormal
                && !matches!(jobs[i].status, JobStatus::Error { .. })
        })
        .min_by(|&a, &b| jobs[a].input.cmp(&jobs[b].input))
}

/// The set's roughness members that are (or will be) written.
fn roughness_members(jobs: &[ResolvedJob], key: &str) -> Vec<usize> {
    (0..jobs.len())
        .filter(|&i| {
            jobs[i].set_key == key
                && jobs[i].profile.subtype.is_roughness_member()
                && writable(&jobs[i])
        })
        .collect()
}

/// Decide every set's bake (spec §6.2–6.3). Attaches `ResolvedJob::bake` to each baking roughness member, records
/// the `_n`'s `mips.normal = renormalize_bake` with provenance `set`, and appends the `SET-*` codes to the jobs they
/// concern. Returns the keys of the sets that bake (`set_resolved.bake`).
pub fn plan_bakes(cx: &BakeCtx, jobs: &mut [ResolvedJob]) -> BTreeSet<String> {
    let batch: BTreeSet<PathBuf> = cx.files.iter().map(|f| f.path.clone()).collect();
    let mut baking = BTreeSet::new();
    // DECISIONS Note R M4-final-1: the `_n` jobs that a borrower set with a roughness member of its own uses. Such an
    // `_n` reports no missing roughness in its own set, whether the borrower bakes or is shared (R7).
    let lent: BTreeSet<usize> = cx
        .borrowed
        .iter()
        .filter(|(b, _)| !roughness_members(jobs, b).is_empty())
        .filter_map(|(_, o)| own_normal(jobs, o))
        .collect();
    for set in cx.sets {
        let idx: Vec<usize> = (0..jobs.len())
            .filter(|&i| jobs[i].set_key == set.key)
            .collect();
        // Spec §10 SET-MISSING-COMPANION, the two triggers that are not about the bake (any target).
        for &i in &idx {
            let lacking = match jobs[i].profile.subtype {
                Subtype::Msn if !set.suffixes.contains("s") => {
                    Some("_msn without an _s (specular) in its set")
                }
                Subtype::Flow if !set.suffixes.contains("n") => {
                    Some("_flow without an _n in its set")
                }
                _ => None,
            };
            // An unavailable sub-type (CFG-UNAVAILABLE) raises nothing else (pre-flight drift 1, controller ruling).
            let unavailable = jobs[i]
                .warnings
                .iter()
                .any(|w| w.code == Code::CfgUnavailable);
            if let (Some(m), true) = (lacking, writable(&jobs[i]) && !unavailable) {
                note(&mut jobs[i], Code::SetMissingCompanion, m.into(), None);
            }
        }
        if cx.target != Target::Cs {
            continue;
        }
        let shared = cx.shared.contains(&set.key);
        // A borrowed Normal slot (GUI) stands in for the set's own `_n` (DECISIONS Note R M4-final-1).
        let owner = cx.borrowed.get(&set.key);
        let normal = match owner {
            Some(o) => own_normal(jobs, o),
            None => own_normal(jobs, &set.key),
        };
        let members = roughness_members(jobs, &set.key);
        match (normal, members.is_empty()) {
            (None, true) => {}
            // M3b spec §3.3 (v1 instruction, DECISIONS R7): a shared roughness renormalises only, silently.
            _ if shared => {}
            (None, false) => match owner.and_then(|o| cx.sets.iter().find(|s| &s.key == o)) {
                Some(os) => {
                    for &m in &members {
                        missing_borrowed_normal(cx, os, &mut jobs[m]);
                    }
                }
                None => {
                    for &m in &members {
                        missing_normal(cx, set, &batch, &mut jobs[m]);
                    }
                }
            },
            (Some(n), true) => {
                if owner.is_none() && !lent.contains(&n) {
                    missing_roughness(cx, set, &batch, &mut jobs[n]);
                }
            }
            (Some(n), false) => {
                if bake_set(jobs, n, &members) {
                    baking.insert(set.key.clone());
                }
            }
        }
    }
    baking
}
