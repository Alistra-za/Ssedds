//! Batch planning of cube jobs (cubemap support v1, DECISIONS DN8): six face files merge into one job, and a single
//! cross, strip or cube DDS gets its layout or an unclear-layout verdict.
use super::{CubeJob, CubeSource, FACES};
use crate::api::SkippedFile;
use crate::codes::{Code, Warning};
use crate::config::{CubeLayout, Subtype};
use crate::group::fold;
use crate::policy::{cube_layout_of, JobStatus, ResolvedJob};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A trailing face tag (`_posx` .. `_negz`, `_px` .. `_nz`, ASCII case-insensitive): the stem without it and the
/// face index (+X -X +Y -Y +Z -Z).
pub fn face_tag(stem: &str) -> Option<(&str, usize)> {
    const TAGS: [(&str, &str); 6] = [
        ("_posx", "_px"),
        ("_negx", "_nx"),
        ("_posy", "_py"),
        ("_negy", "_ny"),
        ("_posz", "_pz"),
        ("_negz", "_nz"),
    ];
    let lower = stem.to_ascii_lowercase();
    TAGS.iter().enumerate().find_map(|(k, (long, short))| {
        [long, short]
            .into_iter()
            .find(|t| lower.len() > t.len() && lower.ends_with(*t))
            .map(|t| (&stem[..stem.len() - t.len()], k))
    })
}

/// The loaded size and cube-DDS flag of each planned input (from its analysis), by path.
pub type InputFacts = BTreeMap<PathBuf, (u32, u32, bool)>;

/// `Error > Blocked > Unresolved > Ready`, codes, reasons and needs united.
fn merge<'a>(all: impl IntoIterator<Item = &'a JobStatus>) -> JobStatus {
    fn add<T: PartialEq + Clone>(to: &mut Vec<T>, from: &[T]) {
        for x in from {
            if !to.contains(x) {
                to.push(x.clone());
            }
        }
    }
    let (mut err, mut blk, mut why, mut need) =
        (None::<Vec<Code>>, None::<Vec<Code>>, None, vec![]);
    for s in all {
        match s {
            JobStatus::Error { codes } => add(err.get_or_insert_with(Vec::new), codes),
            JobStatus::Blocked { codes } => add(blk.get_or_insert_with(Vec::new), codes),
            JobStatus::Unresolved { reasons, needs } => {
                add(why.get_or_insert_with(Vec::new), reasons);
                add(&mut need, needs);
            }
            _ => {}
        }
    }
    match (err, blk, why) {
        (Some(codes), _, _) => JobStatus::Error { codes },
        (_, Some(codes), _) => JobStatus::Blocked { codes },
        (_, _, Some(reasons)) => JobStatus::Unresolved {
            reasons,
            needs: need,
        },
        _ => JobStatus::Ready,
    }
}

/// The '/'-separated game path `p` with the face tag cut from its file stem (the extension kept).
fn untagged(p: &str) -> String {
    let (dir, name) = p.split_at(p.rfind('/').map_or(0, |i| i + 1));
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    match face_tag(stem) {
        Some((base, _)) if ext.is_empty() => format!("{dir}{base}"),
        Some((base, _)) => format!("{dir}{base}.{ext}"),
        None => p.to_string(),
    }
}

/// Merge each set of six `Cubemap` face files (`x_posx` .. `x_negz`, same output folder) into one job, give each
/// other `Cubemap` job its one-file layout, and skip (IN-DUPLICATE) a merged cube whose output another job writes.
/// Runs after pass 1 and the 6c re-resolve, before packing.
pub fn plan_cubes(facts: &InputFacts, jobs: &mut Vec<ResolvedJob>, skipped: &mut Vec<SkippedFile>) {
    let size = |j: &ResolvedJob| facts.get(&j.input).copied().unwrap_or((0, 0, false));
    let mut groups: BTreeMap<(String, String), Vec<(usize, usize)>> = BTreeMap::new();
    let mut singles = vec![];
    for (i, j) in jobs.iter().enumerate() {
        if j.profile.subtype != Subtype::Cubemap {
            continue;
        }
        let (w, h, dds_cube) = size(j);
        let layout = cube_layout_of(&j.profile, &j.input, dds_cube, (w, h));
        let stem = j.output.file_stem().and_then(|s| s.to_str());
        match (layout, stem.and_then(face_tag)) {
            (Some(CubeLayout::Faces), Some((base, face))) => {
                let dir = fold(j.output.parent().unwrap_or(Path::new("")));
                groups
                    .entry((dir, base.to_lowercase()))
                    .or_default()
                    .push((i, face));
            }
            _ => singles.push((i, layout)),
        }
    }
    for (i, layout) in singles {
        let j = &mut jobs[i];
        let (w, h, _) = size(j);
        let verdict = match layout {
            None => JobStatus::Unresolved {
                reasons: vec![format!(
                    "cube layout unclear for {w}x{h}: set cubemap.layout for this file in sdc.toml (hcross, vcross, hstrip, vstrip)"
                )],
                needs: vec![],
            },
            Some(l) if super::layout::cells(l, w, h).is_none() => {
                let msg = if l == CubeLayout::Faces {
                    "cubemap.layout faces is six separate files, not one image".to_string()
                } else {
                    format!("{} layout does not fit {w}x{h}", format!("{l:?}").to_lowercase())
                };
                j.warnings.push(Warning::new(Code::CubeFaces, msg));
                JobStatus::Error {
                    codes: vec![Code::CubeFaces],
                }
            }
            Some(l) => {
                j.cube = Some(CubeJob {
                    layout: l,
                    sources: vec![CubeSource {
                        path: j.input.clone(),
                        stamp: j.source,
                        face: None,
                    }],
                });
                JobStatus::Ready
            }
        };
        j.status = merge([&j.status, &verdict]);
    }

    // One merged job per face group, at the place of its first member; the group's inputs, for a skip.
    let mut merged: BTreeMap<usize, (ResolvedJob, Vec<usize>)> = BTreeMap::new();
    let mut members: BTreeSet<usize> = BTreeSet::new();
    for group in groups.into_values() {
        let first = group[0].0;
        let mut by_face: [Vec<usize>; 6] = Default::default();
        for &(i, f) in &group {
            by_face[f].push(i);
        }
        let missing: Vec<&str> = (0..6)
            .filter(|&f| by_face[f].is_empty())
            .map(|f| FACES[f])
            .collect();
        let fault = if let Some(f) = (0..6).find(|&f| by_face[f].len() > 1) {
            Some(format!("two files for {}", FACES[f]))
        } else if !missing.is_empty() {
            Some(format!("missing {}", missing.join(", ")))
        } else {
            let one: [usize; 6] = std::array::from_fn(|f| by_face[f][0]);
            let dims = one.map(|i| size(&jobs[i]));
            let sha = |i: usize| crate::manifest::profile_sha256(&jobs[i].profile).ok();
            if let Some(f) = (0..6).find(|&f| dims[f].0 != dims[f].1) {
                let (w, h, _) = dims[f];
                Some(format!("{} is not square ({w}x{h})", FACES[f]))
            } else if dims.iter().any(|d| d.0 != dims[0].0) {
                Some("faces differ in size".to_string())
            } else if one.iter().any(|&i| sha(i) != sha(one[0])) {
                Some("faces resolve to different settings".to_string())
            } else {
                None
            }
        };
        let base = match &fault {
            None => by_face[0][0],
            Some(_) => first,
        };
        let mut job = jobs[base].clone();
        if let Some((stem, _)) = job
            .output
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(face_tag)
        {
            job.output.set_file_name(format!("{stem}.dds"));
        }
        job.game_path = untagged(&job.game_path);
        match fault {
            Some(msg) => {
                job.warnings.push(Warning::new(Code::CubeFaces, msg));
                job.status = JobStatus::Error {
                    codes: vec![Code::CubeFaces],
                };
            }
            None => {
                job.status = merge(by_face.iter().map(|v| &jobs[v[0]].status));
                job.cube = Some(CubeJob {
                    layout: CubeLayout::Faces,
                    sources: (0..6)
                        .map(|f| CubeSource {
                            path: jobs[by_face[f][0]].input.clone(),
                            stamp: jobs[by_face[f][0]].source,
                            face: Some(f),
                        })
                        .collect(),
                });
            }
        }
        let ids: Vec<usize> = group.iter().map(|g| g.0).collect();
        members.extend(&ids);
        merged.insert(first, (job, ids));
    }
    if merged.is_empty() {
        return;
    }

    // A merged output was never a discovered file, so `duplicate_outputs_with` could not see it collide.
    let mut out: Vec<(ResolvedJob, Vec<PathBuf>, bool)> = vec![];
    let old = std::mem::take(jobs);
    let inputs: Vec<PathBuf> = old.iter().map(|j| j.input.clone()).collect();
    for (i, j) in old.into_iter().enumerate() {
        if let Some((m, ids)) = merged.remove(&i) {
            out.push((m, ids.iter().map(|&k| inputs[k].clone()).collect(), true));
        } else if !members.contains(&i) {
            let input = j.input.clone();
            out.push((j, vec![input], false));
        }
    }
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for (j, _, _) in &out {
        *count.entry(fold(&j.output)).or_default() += 1;
    }
    let clash: BTreeSet<String> = out
        .iter()
        .filter(|(j, _, m)| *m && count[&fold(&j.output)] > 1)
        .map(|(j, _, _)| fold(&j.output))
        .collect();
    for (j, paths, _) in out {
        if !clash.contains(&fold(&j.output)) {
            jobs.push(j);
            continue;
        }
        for path in paths {
            skipped.push(SkippedFile {
                path,
                output: j.output.clone(),
                set: Some(j.set_key.clone()),
                code: Some(Code::InDuplicate),
                reason: "two inputs map to one output; exclude one".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_cuts_the_stem_tag_only() {
        assert_eq!(
            untagged("textures/a/chrome_e_posx.png"),
            "textures/a/chrome_e.png"
        );
        assert_eq!(untagged("sky_NZ"), "sky");
        assert_eq!(untagged("a_px/x.png"), "a_px/x.png");
    }

    #[test]
    fn merge_ranks_error_blocked_unresolved() {
        let u = JobStatus::Unresolved {
            reasons: vec!["r".into()],
            needs: vec![],
        };
        let b = JobStatus::Blocked {
            codes: vec![Code::DimNpot],
        };
        assert_eq!(merge([&JobStatus::Ready, &u, &u]), u);
        assert_eq!(merge([&u, &b]), b);
        assert_eq!(merge([&JobStatus::Ready]), JobStatus::Ready);
    }
}
