//! What the columns show, built from a plan and its events (M3a spec §2.2–2.3).
use engine::api::Plan;
use engine::codes::{Code, Severity, Warning};
use engine::config::{Subtype, Target};
use engine::discover::{InputFile, InputKind};
use engine::events::{EngineEvent, EventKind, RecipeReport};
use engine::manifest::output_key;
use engine::overrides::project_rel;
use engine::policy::{JobStatus, Need, ResolvedJob};
use engine::session::{Session, Thumbnail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Image,
    Packed,
    Dds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemStatus {
    Ready,
    /// Unresolved or blocked: the amber border (M3a spec §2.2).
    NeedsDecision,
    Error(Vec<Code>),
    Failed(String),
    Skipped(String),
    /// A raw map consumed by a recipe: the packed output's job key.
    PackedInto(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Subtype { suggested: Option<Subtype> },
    AlphaMeaning,
    GreenConvention,
    Acknowledge(Code),
}

#[derive(Debug, Clone)]
pub struct TextureItem {
    /// The `[files]` key: project-relative, packed outputs without extension.
    pub key: String,
    /// The output key (`""` when the item has no output).
    pub job_key: String,
    pub input: PathBuf,
    /// The file name shown under the thumbnail (packed: `<stem>.<recipe>`).
    pub name: String,
    pub set_key: Option<String>,
    pub kind: ItemKind,
    pub job: Option<ResolvedJob>,
    pub status: ItemStatus,
    pub decisions: Vec<Decision>,
    pub warnings: Vec<Warning>,
    pub thumb: Option<Thumbnail>,
    pub changed_on_disk: bool,
}

#[derive(Debug, Clone)]
pub struct SetItem {
    pub key: String,
    /// Item keys.
    pub members: Vec<String>,
    pub recipes: Vec<RecipeReport>,
    pub pbr: bool,
}

#[derive(Debug, Clone)]
pub struct PlanView {
    pub target: Target,
    /// The preset the batch resolved from (the request, the configs, or the target's default).
    pub preset_id: String,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    pub unmatched: Vec<String>,
}

/// The `[files]` key of a path under `root` (DECISIONS N2).
fn key_of(root: &Path, path: &Path) -> String {
    let f = InputFile {
        path: path.to_path_buf(),
        rel: PathBuf::from(path.file_name().unwrap_or_default()),
        game_path: String::new(),
        output: PathBuf::new(),
        kind: InputKind::Image,
    };
    project_rel(root, &f).unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"))
}

/// A packed job's key: its first source's folder plus the output stem, without extension.
fn packed_key(root: &Path, job: &ResolvedJob) -> String {
    let src = key_of(root, &job.input);
    let dir = src
        .rsplit_once('/')
        .map_or(String::new(), |(d, _)| format!("{d}/"));
    format!(
        "{dir}{}",
        job.output.file_stem().unwrap_or_default().to_string_lossy()
    )
}

pub fn decisions_of(job: &ResolvedJob) -> Vec<Decision> {
    let mut out = vec![];
    if let JobStatus::Unresolved { needs, .. } = &job.status {
        for n in needs {
            out.push(match n {
                Need::Subtype => Decision::Subtype {
                    suggested: job.suggestions.first().map(|s| s.subtype),
                },
                Need::AlphaMeaning => Decision::AlphaMeaning,
                Need::GreenConvention => Decision::GreenConvention,
            });
        }
    }
    for w in &job.warnings {
        if w.severity == Severity::Blocking
            && !matches!(w.code, Code::SugLowConfidence | Code::InGreenUnstated)
        {
            out.push(Decision::Acknowledge(w.code));
        }
    }
    out
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

impl PlanView {
    pub fn build(plan: &Plan, events: &[EngineEvent], session: &Session, root: &Path) -> PlanView {
        let out_root = &plan.request.output_root;
        let mut items = vec![];
        for job in &plan.jobs {
            let packed = job.pack.is_some();
            let key = if packed {
                packed_key(root, job)
            } else {
                key_of(root, &job.input)
            };
            let status = match &job.status {
                JobStatus::Ready => ItemStatus::Ready,
                JobStatus::Unresolved { .. } | JobStatus::Blocked { .. } => {
                    ItemStatus::NeedsDecision
                }
                JobStatus::Error { codes } => ItemStatus::Error(codes.clone()),
                JobStatus::PackingInput { role } => {
                    ItemStatus::Skipped(format!("raw {} map, not packed", role.id()))
                }
            };
            let name = if packed {
                job.output
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                file_name(&job.input)
            };
            items.push(TextureItem {
                key,
                job_key: output_key(out_root, &job.output),
                input: job.input.clone(),
                name,
                set_key: Some(job.set_key.clone()),
                kind: if packed {
                    ItemKind::Packed
                } else {
                    ItemKind::Image
                },
                decisions: decisions_of(job),
                warnings: job.warnings.clone(),
                thumb: session.thumbnail(job),
                job: Some(job.clone()),
                status,
                changed_on_disk: false,
            });
            if let Some(pk) = &job.pack {
                for s in &pk.sources {
                    items.push(TextureItem {
                        key: key_of(root, &s.path),
                        job_key: String::new(),
                        input: s.path.clone(),
                        name: file_name(&s.path),
                        set_key: Some(job.set_key.clone()),
                        kind: ItemKind::Image,
                        job: None,
                        status: ItemStatus::PackedInto(output_key(out_root, &job.output)),
                        decisions: vec![],
                        warnings: vec![],
                        thumb: session.cache().thumbnail_of(&s.path),
                        changed_on_disk: false,
                    });
                }
            }
        }
        for s in &plan.skipped {
            let dds = s.code == Some(Code::InDdsSkipped);
            items.push(TextureItem {
                key: key_of(root, &s.path),
                job_key: output_key(out_root, &s.output),
                input: s.path.clone(),
                name: file_name(&s.path),
                set_key: s.set.clone(),
                kind: if dds { ItemKind::Dds } else { ItemKind::Image },
                job: None,
                status: ItemStatus::Skipped(s.reason.clone()),
                decisions: vec![],
                warnings: vec![],
                thumb: None,
                changed_on_disk: false,
            });
        }
        for f in &plan.failed {
            items.push(TextureItem {
                key: key_of(root, &f.path),
                job_key: output_key(out_root, &f.output),
                input: f.path.clone(),
                name: file_name(&f.path),
                set_key: None,
                kind: ItemKind::Image,
                job: None,
                status: ItemStatus::Failed(f.error.clone()),
                decisions: vec![],
                warnings: vec![],
                thumb: None,
                changed_on_disk: false,
            });
        }
        // De-duplicate by key (a raw source can feed two recipes): keep the first.
        let mut seen = std::collections::BTreeSet::new();
        items.retain(|i| seen.insert(i.key.clone()));
        items.sort_by_key(|i| i.key.to_lowercase());

        let by_input: BTreeMap<String, Vec<String>> =
            items.iter().fold(BTreeMap::new(), |mut m, i| {
                m.entry(i.input.to_string_lossy().replace('\\', "/"))
                    .or_default()
                    .push(i.key.clone());
                m
            });
        let mut sets = vec![];
        let mut unmatched = vec![];
        for e in events {
            match &e.kind {
                EventKind::SetResolved {
                    members,
                    pbr,
                    recipes,
                    ..
                } => {
                    let key = e.set.clone().unwrap_or_default();
                    let mut keys: Vec<String> = members
                        .iter()
                        .flat_map(|m| by_input.get(m).cloned().unwrap_or_default())
                        .collect();
                    keys.extend(
                        items
                            .iter()
                            .filter(|i| {
                                i.kind == ItemKind::Packed && i.set_key.as_deref() == Some(&key)
                            })
                            .map(|i| i.key.clone()),
                    );
                    keys.sort();
                    keys.dedup();
                    sets.push(SetItem {
                        key,
                        members: keys,
                        recipes: recipes.clone(),
                        pbr: *pbr,
                    });
                }
                EventKind::Warning(w) if w.code == Code::CfgFileUnmatched => {
                    if let Some(k) = w.data["key"].as_str() {
                        unmatched.push(k.to_string());
                    }
                }
                _ => {}
            }
        }
        PlanView {
            target: plan.target,
            preset_id: plan.preset_id.clone(),
            items,
            sets,
            unmatched,
        }
    }
}
