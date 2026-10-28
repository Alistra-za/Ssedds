//! What the columns show, built from a plan and its events (M3a spec §2.2–2.3).
use engine::api::Plan;
use engine::codes::{Code, Severity, Warning};
use engine::config::{Subtype, Target};
use engine::discover::{InputFile, InputKind};
use engine::events::{EngineEvent, EventKind, RecipeReport};
use engine::helper::report::HelperReport;
use engine::manifest::output_key;
use engine::overrides::project_rel;
use engine::pack::plan::virtual_stem;
use engine::pack::RecipeKind;
use engine::policy::{JobStatus, Need, ResolvedJob, SourceStamp};
use engine::session::{Session, Thumbnail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Image,
    Packed,
    Dds,
    /// Note DX16: a helper file (AO, masks, layers…): a row under the tiles, never a tile.
    Helper,
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
    pub thumb: Option<std::sync::Arc<Thumbnail>>,
    /// The cached analysis, check stats included (DECISIONS Note CL); `None` for skipped, failed and packed-source
    /// items.
    pub analysis: Option<std::sync::Arc<engine::analyse::Analysis>>,
    pub changed_on_disk: bool,
    /// The file's stamp when it was listed. Only DDS inputs carry one: the engine plans no job for them, so
    /// nothing else records what they looked like; an item with a job is checked against the job's stamps.
    pub stamp: Option<SourceStamp>,
    /// Note DX16: the engine's report for a `Helper` item.
    pub helper: Option<std::sync::Arc<HelperReport>>,
}

#[derive(Debug, Clone)]
pub struct SetItem {
    pub key: String,
    /// Item keys.
    pub members: Vec<String>,
    pub recipes: Vec<RecipeReport>,
    pub pbr: bool,
    /// M6: the engine's shader pick and the set's SHADER-* warnings (`SetResolved`).
    pub shader: Option<engine::shader::ShaderPick>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone)]
pub struct PlanView {
    pub target: Target,
    /// The preset the batch resolved from (the request, the configs, or the target's default).
    pub preset_id: String,
    /// `(id, file)` of every user preset the plan's registry loaded.
    pub user_presets: Vec<(String, String)>,
    /// `(file, reason)` of every user preset file the plan skipped (CFG-PRESET-SKIPPED).
    pub skipped_user_presets: Vec<(String, String)>,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    pub unmatched: Vec<String>,
    /// Set key to the RNM-* notes of its layer jobs (a layer file has no item).
    pub rnm_notes: BTreeMap<String, Vec<Warning>>,
    /// Layer, mask and AO path to its `[files]` key.
    pub rnm_keys: BTreeMap<PathBuf, String>,
    /// The export folder the plan ran with: set keys are under it (audit 08c-3).
    pub output_root: PathBuf,
}

/// The `[files]` key of a path under `root` (DECISIONS N2).
pub(crate) fn key_of(root: &Path, path: &Path) -> String {
    let f = InputFile {
        path: path.to_path_buf(),
        rel: PathBuf::from(path.file_name().unwrap_or_default()),
        game_path: String::new(),
        output: PathBuf::new(),
        kind: InputKind::Image,
        real_rel: None,
    };
    project_rel(root, &f).unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"))
}

/// A packed job's key: its first source's folder plus the virtual stem from the first source's name, without
/// extension (as the engine scopes `[files]`; an output path does not change it).
fn packed_key(root: &Path, job: &ResolvedJob, kind: RecipeKind) -> String {
    let src = key_of(root, &job.input);
    let dir = src
        .rsplit_once('/')
        .map_or(String::new(), |(d, _)| format!("{d}/"));
    format!("{dir}{}", virtual_stem(&job.input, kind))
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
            });
        }
    }
    for w in &job.warnings {
        if w.severity == Severity::Blocking && w.code != Code::SugLowConfidence {
            out.push(Decision::Acknowledge(w.code));
        }
    }
    out
}

/// Note DX16: where a helper went, from the engine's report: "fills to x_rmaos R, x alpha" or "not used: <reason>".
pub fn helper_target_text(h: &HelperReport) -> String {
    if h.fills.is_empty() {
        return match h.not_used.as_deref().filter(|r| !r.is_empty()) {
            Some(r) => format!("not used: {r}"),
            None => "not used".to_string(),
        };
    }
    let base = super::helpers::set_base(h);
    let to: Vec<String> = h.fills.iter().map(|t| t.label(&base)).collect();
    format!("fills to {}", to.join(", "))
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
        let mut rnm_notes: BTreeMap<String, Vec<Warning>> = BTreeMap::new();
        let mut rnm_keys = BTreeMap::new();
        // Note DX16: a file with a helper report is one Helper item; its own raw job and its pack-source item are not.
        let helper_paths: std::collections::HashSet<&Path> =
            plan.helpers.iter().map(|h| h.path.as_path()).collect();
        let mut helper_warnings: BTreeMap<&Path, Vec<Warning>> = BTreeMap::new();
        for job in &plan.jobs {
            // M6 Track F2 (Note W11): a layer file has no item; its RNM-* notes are kept for its set.
            if matches!(job.status, JobStatus::NormalLayer { .. }) {
                let notes: Vec<Warning> = job
                    .warnings
                    .iter()
                    .filter(|w| w.code.as_str().starts_with("RNM-"))
                    .cloned()
                    .collect();
                if !notes.is_empty() {
                    rnm_notes
                        .entry(job.set_key.clone())
                        .or_default()
                        .extend(notes);
                }
                continue;
            }
            if matches!(job.status, JobStatus::PackingInput { .. })
                && helper_paths.contains(job.input.as_path())
            {
                // Kept on the helper's item, so PACK-UNUSED-INPUT still reaches Problems.
                helper_warnings
                    .entry(job.input.as_path())
                    .or_default()
                    .extend(job.warnings.iter().cloned());
                continue;
            }
            if let Some(r) = &job.rnm {
                for l in &r.layers {
                    rnm_keys.insert(l.file.path.clone(), key_of(root, &l.file.path));
                    if let Some(m) = &l.mask {
                        rnm_keys.insert(m.path.clone(), key_of(root, &m.path));
                    }
                }
                if let Some(a) = &r.ao {
                    rnm_keys.insert(a.file.path.clone(), key_of(root, &a.file.path));
                }
            }
            let packed = job.pack.is_some();
            let key = if let Some(pk) = &job.pack {
                packed_key(root, job, pk.kind)
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
                JobStatus::NormalLayer { .. } => unreachable!("layer jobs are skipped above"),
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
                analysis: session.cache().analysis(job).map(std::sync::Arc::new),
                job: Some(job.clone()),
                status,
                changed_on_disk: false,
                stamp: None,
                helper: None,
            });
            if let Some(pk) = &job.pack {
                for s in pk
                    .sources
                    .iter()
                    .filter(|s| !helper_paths.contains(s.path.as_path()))
                {
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
                        analysis: None,
                        changed_on_disk: false,
                        stamp: None,
                        helper: None,
                    });
                }
            }
        }
        // First, so the de-duplication below keeps the helper over any other item for its file.
        let mut helpers: Vec<TextureItem> = plan
            .helpers
            .iter()
            .map(|h| {
                let failed = plan.failed.iter().find(|f| f.path == h.path);
                let status = match (failed, h.outputs.first()) {
                    (Some(f), _) => ItemStatus::Failed(f.error.clone()),
                    (None, Some(o)) => ItemStatus::PackedInto(output_key(out_root, o)),
                    (None, None) => ItemStatus::Skipped(h.not_used.clone().unwrap_or_default()),
                };
                TextureItem {
                    key: key_of(root, &h.path),
                    job_key: String::new(),
                    input: h.path.clone(),
                    name: file_name(&h.path),
                    set_key: Some(h.set_key.clone()),
                    kind: ItemKind::Helper,
                    job: None,
                    status,
                    decisions: vec![],
                    warnings: helper_warnings.remove(h.path.as_path()).unwrap_or_default(),
                    thumb: session.cache().thumbnail_of(&h.path),
                    analysis: None,
                    changed_on_disk: false,
                    stamp: None,
                    helper: Some(std::sync::Arc::new(h.clone())),
                }
            })
            .collect();
        helpers.append(&mut items);
        let mut items = helpers;
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
                // Preflight P4: GEN-REPLACED stays on the item (Info: never a Problems row) for the rival's name.
                warnings: match s.code {
                    Some(c @ Code::GenReplaced) => vec![Warning::new(c, s.reason.clone())],
                    _ => vec![],
                },
                thumb: None,
                analysis: None,
                changed_on_disk: false,
                stamp: if dds { SourceStamp::of(&s.path) } else { None },
                helper: None,
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
                analysis: None,
                changed_on_disk: false,
                stamp: None,
                helper: None,
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
        let mut skipped_user_presets = vec![];
        for e in events {
            match &e.kind {
                EventKind::SetResolved {
                    members,
                    pbr,
                    recipes,
                    shader,
                    warnings,
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
                        shader: shader.clone(),
                        warnings: warnings.clone(),
                    });
                }
                EventKind::Warning(w) if w.code == Code::CfgFileUnmatched => {
                    if let Some(k) = w.data["key"].as_str() {
                        unmatched.push(k.to_string());
                    }
                }
                EventKind::Warning(w) if w.code == Code::CfgPresetSkipped => {
                    let s = |k: &str| w.data[k].as_str().unwrap_or_default().to_string();
                    skipped_user_presets.push((s("file"), s("reason")));
                }
                _ => {}
            }
        }
        PlanView {
            target: plan.target,
            preset_id: plan.preset_id.clone(),
            user_presets: plan
                .registry
                .user_presets()
                .into_iter()
                .map(|(id, src)| (id.to_string(), src.to_string()))
                .collect(),
            skipped_user_presets,
            items,
            sets,
            unmatched,
            rnm_notes,
            rnm_keys,
            output_root: out_root.clone(),
        }
    }
}
