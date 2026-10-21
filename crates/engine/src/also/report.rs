//! C5 (Note CM): `<location>/<project name> conversion-report.md`, written last by every Also export that is not a
//! dry run, a cancelled or failed one included. LF endings and no timestamp: the same inputs give the same bytes.
use super::{cm_lines, AlsoSpec, AlsoSummary, Phase, SetNote};
use crate::api::Plan;
use crate::codes::Code;
use crate::derive::{DeriveKind, DeriveSet};
use crate::policy::{JobStatus, Provenance, ResolvedJob};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

/// One PBR set's line in the report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetRow {
    pub id: String,
    /// TA16, from `DERIVE-NIF-SUGGEST`; `None` for landscape and grass (no suggestion).
    pub glossiness: Option<f64>,
    pub specular_strength: Option<f64>,
    /// The Environment Map shader is needed (the set has metal).
    pub env_map: Option<bool>,
    pub env_m_written: bool,
    pub notes: Vec<String>,
}

/// TA26.
pub const TOKSVIG: &str =
    "Toksvig not applied: the CM `_m` is not gloss-baked, although the `_n` opts in (renormalize_bake)";
/// TA27.
pub const INNER_LAYER: &str = "inner-layer `_p` copied unchanged";

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// I1: a plan-time error job of a derived output: a derive recipe's (`recipe:<kind>`) on a member of a PBR set.
pub(crate) fn derive_error(j: &ResolvedJob, members: &BTreeSet<&Path>) -> bool {
    const KINDS: [DeriveKind; 6] = [
        DeriveKind::VanillaDiffuse,
        DeriveKind::CmDiffuse,
        DeriveKind::LegacyN,
        DeriveKind::CmM,
        DeriveKind::EnvM,
        DeriveKind::GrassStack,
    ];
    matches!(j.status, JobStatus::Error { .. })
        && members.contains(j.input.as_path())
        && matches!(j.provenance.get("subtype"), Some(Provenance::Recipe(id)) if KINDS.iter().any(|k| k.id() == id))
}

fn is(j: &ResolvedJob, k: DeriveKind) -> bool {
    j.derive.as_ref().is_some_and(|d| d.kind == k)
}

/// The rows of `sets` from the Vanilla plan, read before it runs (Task 4 puts the NIF suggestion and the
/// not-converted notes there only). A set the plan skipped whole (`DERIVE-DDS-SOURCE`, `DERIVE-NO-ALBEDO`) gives a
/// `SetNote` instead of a row. A member reported both as a set-level `DERIVE-NOT-CONVERTED` note and as a per-file
/// skip is listed once, with the note's reason (it carries TA28's). `toksvig`: the CM folder is written (TA26).
pub(crate) fn vanilla_rows(
    plan: &Plan,
    sets: &[DeriveSet],
    toksvig: bool,
    inner: &BTreeSet<String>,
) -> (Vec<SetRow>, Vec<SetNote>) {
    let owner: BTreeMap<&Path, usize> = sets
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.members.iter().map(move |m| (m.as_path(), i)))
        .collect();
    let mut rows: Vec<SetRow> = sets
        .iter()
        .map(|s| SetRow {
            id: s.id.clone(),
            ..SetRow::default()
        })
        .collect();
    let mut members: Vec<BTreeMap<String, String>> = vec![BTreeMap::new(); sets.len()];
    let mut whole: Vec<Option<String>> = vec![None; sets.len()];
    for j in &plan.jobs {
        let Some(&i) = owner.get(j.input.as_path()) else {
            continue;
        };
        let r = &mut rows[i];
        let error = matches!(j.status, JobStatus::Error { .. });
        r.env_m_written |= is(j, DeriveKind::EnvM) && !error;
        if toksvig && is(j, DeriveKind::LegacyN) && crate::bake::plan::gloss_bake_opted(j) {
            r.notes.push(TOKSVIG.into());
        }
        for w in &j.warnings {
            match w.code {
                Code::DeriveNifSuggest => {
                    r.glossiness = w.data["glossiness"].as_f64();
                    r.specular_strength = w.data["specular_strength"].as_f64();
                    r.env_map = w.data["env_map"].as_bool();
                }
                Code::DeriveNotConverted => {
                    let m = w.data["member"].as_str().unwrap_or(&w.message);
                    members[i].insert(m.to_string(), w.message.clone());
                }
                c if error || c.as_str().starts_with("DERIVE-") => {
                    r.notes.push(format!("{}: {}", c.as_str(), w.message))
                }
                _ => {}
            }
        }
    }
    for f in &plan.failed {
        if let Some(&i) = owner.get(f.path.as_path()) {
            rows[i]
                .notes
                .push(format!("{} failed: {}", slash(&f.path), f.error));
        }
    }
    for k in &plan.skipped {
        let Some(&i) = owner.get(k.path.as_path()) else {
            continue;
        };
        match k.code {
            Some(Code::DeriveDdsSource | Code::DeriveNoAlbedo) => whole[i] = Some(k.reason.clone()),
            Some(Code::DeriveNotConverted) => {
                members[i]
                    .entry(slash(&k.path))
                    .or_insert_with(|| k.reason.clone());
            }
            _ => {}
        }
    }
    let inner: BTreeSet<String> = inner.iter().map(|s| crate::sets::fold_id(s)).collect();
    let (mut out, mut skipped) = (vec![], vec![]);
    for ((mut r, m), why) in rows.into_iter().zip(members).zip(whole) {
        if let Some(why) = why {
            skipped.push(SetNote { id: r.id, why });
            continue;
        }
        r.notes.extend(m.into_values());
        if inner.contains(&crate::sets::fold_id(&r.id)) {
            r.notes.push(INNER_LAYER.into());
        }
        dedup(&mut r.notes);
        out.push(r);
    }
    (out, skipped)
}

/// The CM plan's derived error jobs (a size mismatch, a path conflict) on the rows; a plain error job is the
/// Vanilla phase's already (review M2).
pub(crate) fn add_cm_errors(rows: &mut [SetRow], plan: &Plan, sets: &[DeriveSet]) {
    let members: BTreeSet<&Path> = sets
        .iter()
        .flat_map(|s| s.members.iter().map(|m| m.as_path()))
        .collect();
    for j in plan.jobs.iter().filter(|j| derive_error(j, &members)) {
        let Some(set) = sets.iter().find(|s| s.members.contains(&j.input)) else {
            continue;
        };
        if let Some(r) = rows.iter_mut().find(|r| r.id == set.id) {
            r.notes.extend(
                j.warnings
                    .iter()
                    .map(|w| format!("CM: {}: {}", w.code.as_str(), w.message)),
            );
            dedup(&mut r.notes);
        }
    }
}

/// Removes repeats, keeping the first of each.
fn dedup(v: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    v.retain(|n| seen.insert(n.clone()));
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Pbr => "PBR",
        Phase::Vanilla => "Vanilla",
        Phase::Cm => "CM",
    }
}

/// The report's Markdown. `grass`: a PBR grass set was exported, so the CM text names the complex-grass stacks.
pub fn render(s: &AlsoSummary, rows: &[SetRow], spec: &AlsoSpec, grass: bool) -> String {
    let mut o = String::from("<!-- written by Tessalith; regenerated on every Also export -->\n");
    o.push_str("# Conversion report\n\n## Folders\n\n");
    o.push_str("| Folder | Converted | Copied | Up to date | Skipped | Failed |\n");
    o.push_str("|---|---|---|---|---|---|\n");
    for (p, b) in &s.phases {
        let name = s.folders[*p as usize]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let copied = match p {
            Phase::Vanilla => s.copied.to_string(),
            _ => "-".into(),
        };
        let _ = writeln!(
            o,
            "| {name} | {} | {copied} | {} | {} | {} |",
            b.converted, b.up_to_date, b.skipped, b.failed
        );
    }
    if let Some(p) = s.cancelled_after {
        let _ = writeln!(
            o,
            "\nCancelled after {}: the later folders were not written.",
            phase_name(p)
        );
    }
    if let Some((p, msg, code)) = &s.error {
        let _ = writeln!(
            o,
            "\nStopped: the {} phase failed: {msg} (exit code {code}).",
            phase_name(*p)
        );
    }

    o.push_str("\n## Settings\n\n```toml\n[export.legacy]\n");
    o.push_str(&toml::to_string(&spec.cfg.legacy).unwrap_or_default());
    o.push_str("```\n\n");
    let _ = writeln!(
        o,
        "Mesh scan read: {}.",
        if spec.scanned { "yes" } else { "no" }
    );

    o.push_str("\n## PBR sets\n\n");
    if rows.is_empty() && (s.cancelled_after.is_some() || s.error.is_some()) {
        // Review M1: the rows come from the Vanilla plan, which a cancel or an error may have stopped first.
        o.push_str(
            "No set rows: the export stopped before the Vanilla phase planned the PBR sets.\n",
        );
    } else if rows.is_empty() {
        o.push_str("No PBR set was converted.\n");
    } else {
        o.push_str("| Set | Suggested NIF Glossiness | Specular Strength | Environment Map shader needed | env `_m` written |\n");
        o.push_str("|---|---|---|---|---|\n");
        for r in rows {
            let gloss = r.glossiness.map_or("-".into(), |g| format!("{g:.1}"));
            let spec_s = r.specular_strength.map_or("-".into(), |v| format!("{v}"));
            let env = match r.env_map {
                Some(true) => "yes",
                Some(false) => "no",
                None => "-",
            };
            let env_m = match (r.env_m_written, r.env_map, spec.scanned) {
                (true, ..) => "yes",
                (false, Some(true), false) => "no: no scan",
                (false, Some(true), true) => "no: mesh not env-map",
                _ => "no",
            };
            let _ = writeln!(o, "| {} | {gloss} | {spec_s} | {env} | {env_m} |", r.id);
        }
        for r in rows.iter().filter(|r| !r.notes.is_empty()) {
            let _ = writeln!(o, "\n### {}\n", r.id);
            for n in &r.notes {
                let _ = writeln!(o, "- {n}");
            }
        }
    }
    if !s.sets.is_empty() {
        o.push_str("\n## Sets not converted\n\n");
        for n in &s.sets {
            let _ = writeln!(o, "- {}: {}", n.id, n.why);
        }
    }
    // Review M3: only when a CM folder was written.
    if s.phases.iter().any(|(p, _)| *p == Phase::Cm) {
        o.push_str("\n## Install\n\n");
        let cm = format!("{}/", super::folder_name(&s.folders[2]));
        let stale = s.notes.iter().any(|n| {
            n.starts_with(&cm)
                && (n.ends_with(super::NOT_WRITTEN) || n.ends_with(super::NOT_ZIPPED))
        });
        o.push_str(&cm_lines(&spec.cfg, grass, stale).join(" "));
        o.push('\n');
    }
    if !s.notes.is_empty() {
        o.push_str("\n## Notes\n\n");
        for n in &s.notes {
            let _ = writeln!(o, "- {n}");
        }
    }
    o
}
