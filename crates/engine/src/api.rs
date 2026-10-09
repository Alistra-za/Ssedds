//! Engine API (spec §2, §5.9, §8): `Engine::plan(BatchRequest) -> Plan`, then `Engine::run(Plan, events, cancel)`.
//!
//! `plan` runs Discover → Group → Load → Analyse → Resolve and reports every decision; nothing is written.
//! `run` applies `--on-existing`, then converts the ready files on a rayon pool of `jobs` threads, admitting them
//! by estimated memory against `--mem-budget`. Files that are unresolved, blocked or in error are reported and
//! never converted: the engine does not guess (spec §8, DECISIONS "never auto-decide").
use crate::analyse::analyse;
use crate::codes::{Code, Warning};
use crate::config::*;
use crate::discover::{canonicalise_outputs, discover, InputFile, InputKind};
use crate::events::{EventKind, Reporter, Stage};
use crate::group::{group_sets_with, split_suffix_with, TextureSet};
use crate::load::{load_image, LoadError};
use crate::manifest::{
    output_key, sha256_file, sha256_json, EncoderVersions, Manifest, ManifestEntry, PresetRef,
    ENGINE_VERSION,
};
use crate::overrides::{to_patch, Scope, SetOverride};
use crate::pipeline::{process_file, FileError};
use crate::policy::*;
use crate::presets::*;
use crate::suggest::{is_pbr_set, SuggestContext};
use encoders_sys::Bc1Model;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// What `run` does when a ready file's output already exists (spec §3.2 `output.on_existing`, §8). It is a batch
/// setting, not a `Profile` field.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum OnExisting {
    /// Re-encode when the manifest shows a different source, profile, engine or encoder; keep an output that has
    /// no manifest record (`OUT-UNKNOWN-EXISTING`).
    #[default]
    IfChanged,
    Skip,
    Overwrite,
}

/// One batch, as the CLI or GUI hands it to the engine. `output_root` and `on_existing` are the spec §3.2
/// `output {path, on_existing}` batch settings.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub inputs: Vec<PathBuf>,
    pub output_root: PathBuf,
    /// `--target`. `None` lets the project config, then the user config, then the selected preset decide, and
    /// finally `cs`; the origin is recorded as `provenance["target"]` of every job.
    pub target: Option<Target>,
    /// Preset id; `None` takes the configs' `preset`, then `default_preset_for(target)`.
    pub preset: Option<String>,
    /// Preset files to load (not select), e.g. ones declaring suffix aliases.
    pub preset_files: Vec<PathBuf>,
    /// User config; skipped when the file does not exist.
    pub user_config: Option<PathBuf>,
    /// Project config; defaults to `sdc.toml` in the first input folder when that exists.
    pub project_config: Option<PathBuf>,
    pub overrides: Vec<SetOverride>,
    pub accept: AcceptSuggestions,
    pub allow: BTreeSet<Code>,
    pub recursive: bool,
    pub on_existing: OnExisting,
    /// Worker threads; 0 = one per logical core.
    pub jobs: usize,
    /// Memory budget in bytes for admitting files; 0 = half of physical RAM.
    pub mem_budget: u64,
    pub bc1_model: Bc1Model,
    /// Also write `<name>.dds.json` next to each output.
    pub sidecar: bool,
    pub dry_run: bool,
}

impl BatchRequest {
    pub fn new(inputs: Vec<PathBuf>, output_root: PathBuf, target: Target) -> Self {
        BatchRequest {
            inputs,
            output_root,
            target: Some(target),
            preset: None,
            preset_files: vec![],
            user_config: None,
            project_config: None,
            overrides: vec![],
            accept: AcceptSuggestions::None,
            allow: BTreeSet::new(),
            recursive: false,
            on_existing: OnExisting::IfChanged,
            jobs: 0,
            mem_budget: 0,
            bc1_model: Bc1Model::Ideal,
            sidecar: false,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub code: Option<Code>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct FailedFile {
    pub path: PathBuf,
    pub stage: Stage,
    pub error: String,
}

pub struct Plan {
    pub request: BatchRequest,
    pub registry: PresetRegistry,
    pub preset_id: String,
    /// The batch target as resolved from `request.target`, the configs or the preset.
    pub target: Target,
    pub jobs: Vec<ResolvedJob>,
    /// Files reported before Resolve: `IN-DUPLICATE` and `IN-DDS-SKIPPED`.
    pub skipped: Vec<SkippedFile>,
    /// Files that could not be loaded (`DIM-TOO-LARGE`, `IN-DECODE`).
    pub failed: Vec<FailedFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BatchSummary {
    /// Written outputs, including those whose validation failed.
    pub converted: u32,
    pub up_to_date: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Written outputs whose validation reported an error (`passed = false`); also counted in `converted`.
    pub validation_failed: u32,
    /// Sorted.
    pub cancelled: Vec<PathBuf>,
}

impl BatchSummary {
    /// Spec §8 precedence for a batch that ran: 1 partial (skipped, failed or cancelled files) > 4 validation
    /// failed > 0. Up-to-date files and raw packing inputs are not skipped files. (2 and 3 come from
    /// `EngineError`.)
    pub fn exit_code(&self) -> i32 {
        if self.skipped + self.failed > 0 || !self.cancelled.is_empty() {
            1
        } else if self.validation_failed > 0 {
            4
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Bad flag, `--set` value or input path (exit 2).
    #[error("{0}")]
    Usage(String),
    /// Invalid preset or config file, including `PresetError::Alias` (`CFG-INVALID`: a bad or conflicting suffix
    /// alias, or the reserved `sdc-base` id) (exit 2).
    #[error("{0}")]
    Config(String),
    /// Output-root I/O, encoder initialisation, internal error (exit 3).
    #[error("{0}")]
    Fatal(String),
}

impl EngineError {
    pub fn exit_code(&self) -> i32 {
        match self {
            EngineError::Usage(_) | EngineError::Config(_) => 2,
            EngineError::Fatal(_) => 3,
        }
    }
}

/// A preset or config file error.
fn config_err(e: PresetError) -> EngineError {
    EngineError::Config(e.to_string())
}

/// A preset error reached through a flag (`--preset <id>`): `Alias` stays a config error.
fn preset_err(e: PresetError) -> EngineError {
    match e {
        PresetError::Alias(m) => EngineError::Config(m),
        e => EngineError::Usage(e.to_string()),
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Memory admission (spec §5.9). Only the scheduling thread acquires; workers release.
struct Budget {
    limit: u64,
    used: Mutex<u64>,
    cv: Condvar,
}

impl Budget {
    /// Waits until `n` more bytes fit. A file larger than the whole budget is admitted alone.
    fn acquire(&self, n: u64) {
        let mut used = lock(&self.used);
        while *used > 0 && used.saturating_add(n) > self.limit {
            used = self.cv.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += n;
    }
    fn release(&self, n: u64) {
        *lock(&self.used) -= n;
        self.cv.notify_all();
    }
}

/// Releases a file's admitted bytes when dropped, so a panic cannot leak budget.
struct Admitted<'a>(&'a Budget, u64);

impl Drop for Admitted<'_> {
    fn drop(&mut self) {
        self.0.release(self.1);
    }
}

/// Peak bytes one file holds while converting: the float base plus its mip chain (about 1.34x), counted three
/// times for the loaded image, the chain and the encoder inputs, decoded validation levels and output bytes.
fn estimated_bytes(path: &Path) -> u64 {
    image::image_dimensions(path)
        .map(|(w, h)| u64::from(w) * u64::from(h) * 16 * 134 / 100 * 3)
        .unwrap_or(0)
}

fn thread_pool(jobs: usize) -> Result<rayon::ThreadPool, EngineError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .thread_name(|i| format!("sdc-worker-{i}"))
        .build()
        .map_err(|e| EngineError::Fatal(format!("thread pool: {e}")))
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The batch target and where it came from: `--target`, the project config, the user config, the selected
/// preset's chain (nearest preset first), else the built-in `cs`.
fn resolve_target(
    req: &BatchRequest,
    project: Option<&ConfigFile>,
    user: Option<&ConfigFile>,
    preset_chain: &[&PresetFile],
) -> (Target, Provenance) {
    if let Some(t) = req.target {
        return (t, Provenance::User);
    }
    if let Some(t) = project.and_then(|c| c.target) {
        return (t, Provenance::ProjectConfig);
    }
    if let Some(t) = user.and_then(|c| c.target) {
        return (t, Provenance::UserConfig);
    }
    for p in preset_chain.iter().rev() {
        if let Some(t) = p.target {
            let src = if p.id == BASE_ID {
                Provenance::Builtin
            } else {
                Provenance::Preset(p.id.clone())
            };
            return (t, src);
        }
    }
    (Target::Cs, Provenance::Builtin)
}

/// The skip code of an unresolved job: its `SUG-LOW-CONFIDENCE` or `IN-GREEN-UNSTATED` warning.
fn unresolved_code(job: &ResolvedJob) -> Code {
    job.warnings
        .iter()
        .map(|w| w.code)
        .find(|c| matches!(c, Code::SugLowConfidence | Code::InGreenUnstated))
        .unwrap_or(Code::SugLowConfidence)
}

/// Whether a manifest entry proves the existing output is current (spec §8 `if-changed`, carry 13): same source,
/// profile, engine version and encoders, a comparable record (known schema, encoders and preset present) and a
/// passing validation, so a failed output is re-encoded and reported again rather than counted as up to date.
fn is_up_to_date(e: &ManifestEntry, job: &ResolvedJob, encoders: &EncoderVersions) -> bool {
    if e.is_newer_schema() || e.preset.is_none() || e.encoders.as_ref() != Some(encoders) {
        return false;
    }
    let passed = e
        .raw
        .get("validation")
        .and_then(|v| v.get("passed"))
        .and_then(Value::as_bool)
        == Some(true);
    passed
        && e.engine_version == ENGINE_VERSION
        && e.sources.is_empty()
        && sha256_json(&job.profile).is_ok_and(|h| h == e.profile_sha256)
        && sha256_file(&job.input).is_ok_and(|h| h == e.source_sha256)
}

pub struct Engine {
    registry: PresetRegistry,
}

impl Engine {
    pub fn new() -> Result<Self, EngineError> {
        Ok(Engine {
            registry: PresetRegistry::builtin().map_err(|e| EngineError::Fatal(e.to_string()))?,
        })
    }

    pub fn registry(&self) -> &PresetRegistry {
        &self.registry
    }

    /// Discover, group, load, analyse and resolve every input, emitting `batch_started`, `file_discovered`,
    /// `set_resolved`, `file_planned` and every warning. Presets, configs, rules and `--set` values are
    /// validated first: any invalid one is an error (exit 2) before any file is read.
    pub fn plan(&self, req: BatchRequest, rep: &Reporter) -> Result<Plan, EngineError> {
        let mut registry = self.registry.clone();
        for f in &req.preset_files {
            registry.add_file(f).map_err(config_err)?;
        }
        let user_config = req
            .user_config
            .as_deref()
            .filter(|p| p.exists())
            .map(ConfigFile::load)
            .transpose()
            .map_err(config_err)?;
        let project_path = req.project_config.clone().or_else(|| {
            req.inputs
                .first()
                .filter(|p| p.is_dir())
                .map(|p| p.join("sdc.toml"))
                .filter(|p| p.exists())
        });
        let project_config = project_path
            .as_deref()
            .map(ConfigFile::load)
            .transpose()
            .map_err(config_err)?;
        let configs: Vec<&ConfigFile> = user_config.iter().chain(project_config.iter()).collect();

        // Target and preset. An explicitly chosen preset may state the target; otherwise the target picks the
        // default preset.
        let chosen = req
            .preset
            .clone()
            .or_else(|| project_config.as_ref().and_then(|c| c.preset.clone()))
            .or_else(|| user_config.as_ref().and_then(|c| c.preset.clone()));
        let chosen_chain = match &chosen {
            Some(id) => registry.chain(id).map_err(preset_err)?,
            None => vec![],
        };
        let (target, target_src) = resolve_target(
            &req,
            project_config.as_ref(),
            user_config.as_ref(),
            &chosen_chain,
        );
        let preset_id = chosen.unwrap_or_else(|| default_preset_for(target).to_string());
        let chain = registry.chain(&preset_id).map_err(preset_err)?;

        check_up_front(&req, &registry, target, &chain, &configs)?;

        for i in &req.inputs {
            if !i.exists() {
                return Err(EngineError::Usage(format!(
                    "input not found: {}",
                    i.display()
                )));
            }
        }
        let mut files = discover(&req.inputs, &req.output_root, req.recursive)
            .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
        // Task 26a: canonical output names before grouping, so alias/canonical collisions are IN-DUPLICATE.
        canonicalise_outputs(&mut files, registry.alias_table());
        rep.emit(
            None,
            None,
            EventKind::BatchStarted {
                target,
                preset: preset_id.clone(),
                inputs: files.len(),
            },
        );
        for f in &files {
            rep.emit(
                None,
                Some(&f.path),
                EventKind::FileDiscovered {
                    output: slash(&f.output),
                },
            );
        }
        let (sets, dups) = group_sets_with(&files, registry.alias_table());
        let dup_idx: BTreeSet<usize> = dups.iter().flatten().copied().collect();
        let mut skipped: Vec<SkippedFile> = dup_idx
            .iter()
            .map(|&i| SkippedFile {
                path: files[i].path.clone(),
                code: Some(Code::InDuplicate),
                reason: "two inputs map to one output; exclude one".into(),
            })
            .collect();
        let mut work: Vec<(&TextureSet, &InputFile)> = vec![];
        for set in &sets {
            for &i in &set.members {
                if dup_idx.contains(&i) {
                    continue;
                }
                if files[i].kind == InputKind::Dds {
                    skipped.push(SkippedFile {
                        path: files[i].path.clone(),
                        code: Some(Code::InDdsSkipped),
                        reason: "DDS inputs are not re-encoded by default".into(),
                    });
                    continue;
                }
                work.push((set, &files[i]));
            }
        }

        let ctx = ResolveCtx {
            target,
            registry: &registry,
            preset: &preset_id,
            user_config: user_config.as_ref(),
            project_config: project_config.as_ref(),
            overrides: &req.overrides,
            accept: req.accept,
            allow: &req.allow,
        };
        // Load and Analyse in parallel; only the `Analysis` is kept, so planning holds at most one decoded image
        // per worker thread.
        let resolve_one =
            |&(set, f): &(&TextureSet, &InputFile)| -> Result<ResolvedJob, (PathBuf, LoadError)> {
                let (img, info) = load_image(&f.path).map_err(|e| (f.path.clone(), e))?;
                let a = analyse(&img, &info);
                drop(img);
                let stem = f
                    .rel
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let split = split_suffix_with(&stem, registry.alias_table());
                let facts = FileFacts {
                    input: f,
                    suffix: split.suffix.as_deref(),
                    alias: split.alias.as_deref(),
                    set,
                    analysis: &a,
                    info: &info,
                };
                let mut job = resolve_file(&ctx, &facts);
                job.provenance.insert("target".into(), target_src.clone());
                Ok(job)
            };
        let results: Vec<_> =
            thread_pool(req.jobs)?.install(|| work.par_iter().map(resolve_one).collect());

        let mut jobs = vec![];
        let mut failed = vec![];
        for r in results {
            match r {
                Ok(job) => jobs.push(job),
                Err((path, e)) => {
                    let code = match e {
                        LoadError::TooLarge { .. } => Code::DimTooLarge,
                        LoadError::Decode { .. } => Code::InDecode,
                    };
                    rep.emit(
                        None,
                        Some(&path),
                        EventKind::Warning(Warning::new(code, e.to_string())),
                    );
                    failed.push(FailedFile {
                        path,
                        stage: Stage::Load,
                        error: e.to_string(),
                    });
                }
            }
        }
        for set in &sets {
            let Some(&first) = set
                .members
                .iter()
                .find(|&&i| files[i].kind == InputKind::Image)
            else {
                continue;
            };
            let pbr = is_pbr_set(&SuggestContext {
                target,
                suffix: None,
                game_path: &files[first].game_path,
                set_suffixes: &set.suffixes,
                alpha: AlphaContent::None,
                is_float: false,
            });
            let members = set.members.iter().map(|&m| slash(&files[m].path)).collect();
            rep.emit(
                Some(&set.key),
                None,
                EventKind::SetResolved {
                    members,
                    pbr,
                    // The variance bake arrives with milestone M4.
                    bake: false,
                },
            );
        }
        for job in &jobs {
            rep.emit(
                Some(&job.set_key),
                Some(&job.input),
                EventKind::FilePlanned {
                    job: serde_json::to_value(job).unwrap_or_default(),
                },
            );
            for w in &job.warnings {
                rep.emit(
                    Some(&job.set_key),
                    Some(&job.input),
                    EventKind::Warning(w.clone()),
                );
            }
        }
        Ok(Plan {
            request: req,
            registry,
            preset_id,
            target,
            jobs,
            skipped,
            failed,
        })
    }

    /// Report the plan's skipped and failed files, then (unless `dry_run`) convert every ready file and emit
    /// `batch_done`. Cancelled files are listed in `batch_done` and leave no output.
    pub fn run(
        &self,
        plan: Plan,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<BatchSummary, EngineError> {
        let req = &plan.request;
        let mut summary = BatchSummary {
            failed: plan.failed.len() as u32,
            ..Default::default()
        };
        let skip = |path: &Path, code: Option<Code>, reason: String, s: &mut BatchSummary| {
            rep.emit(None, Some(path), EventKind::FileSkipped { code, reason });
            s.skipped += 1;
        };
        for s in &plan.skipped {
            skip(&s.path, s.code, s.reason.clone(), &mut summary);
        }
        for f in &plan.failed {
            rep.emit(
                None,
                Some(&f.path),
                EventKind::FileFailed {
                    stage: f.stage,
                    error: f.error.clone(),
                },
            );
        }
        let mut ready = vec![];
        for job in &plan.jobs {
            let (set, path) = (Some(job.set_key.as_str()), Some(job.input.as_path()));
            match &job.status {
                JobStatus::Ready => ready.push(job),
                JobStatus::Unresolved { reasons } => {
                    skip(&job.input, Some(unresolved_code(job)), reasons.join("; "), &mut summary)
                }
                // Spec §4.4/§8 (Task 27a): a raw packing input is reported but does not make the batch partial.
                JobStatus::PackingInput { role } => rep.emit(
                    set,
                    path,
                    EventKind::FileSkipped {
                        code: Some(Code::InPackingInput),
                        reason: format!(
                            "raw {} map: a channel-packing input (milestone M2.5), not converted on its own",
                            role.id()
                        ),
                    },
                ),
                JobStatus::Blocked { codes } => skip(
                    &job.input,
                    codes.first().copied(),
                    format!(
                        "blocked by {}; acknowledge with --allow <code> or change the setting",
                        codes.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                    &mut summary,
                ),
                // Spec §10: an error-severity code fails the file.
                JobStatus::Error { codes } => {
                    summary.failed += 1;
                    rep.emit(
                        set,
                        path,
                        EventKind::FileFailed {
                            stage: Stage::Resolve,
                            error: format!(
                                "resolve error: {}",
                                codes.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
                            ),
                        },
                    );
                }
            }
        }
        if req.dry_run {
            rep.emit(None, None, done_event(&summary));
            return Ok(summary);
        }

        // Carry 8: the BC1 model is global rgbcx state; set it once before any encode, decode or validate.
        encoders_sys::init(req.bc1_model).map_err(|e| EngineError::Fatal(e.to_string()))?;
        std::fs::create_dir_all(&req.output_root).map_err(|e| {
            EngineError::Fatal(format!("output root {}: {e}", req.output_root.display()))
        })?;
        let manifest = Manifest::at(&req.output_root);
        let existing = manifest.load().map_err(|e| {
            EngineError::Fatal(format!("manifest {}: {e}", manifest.path().display()))
        })?;
        let encoders = EncoderVersions::current();

        let mut todo: Vec<(&ResolvedJob, PresetRef)> = vec![];
        for job in ready {
            if job.output.exists() {
                match req.on_existing {
                    OnExisting::Overwrite => {}
                    OnExisting::Skip => {
                        skip(
                            &job.input,
                            None,
                            "output exists (--on-existing skip)".into(),
                            &mut summary,
                        );
                        continue;
                    }
                    OnExisting::IfChanged => {
                        match existing.get(&output_key(&req.output_root, &job.output)) {
                            None => {
                                rep.emit(
                                Some(&job.set_key),
                                Some(&job.input),
                                EventKind::Warning(Warning::new(
                                    Code::OutUnknownExisting,
                                    format!(
                                        "{} exists without a manifest record; kept (use --on-existing overwrite)",
                                        slash(&job.output)
                                    ),
                                )),
                            );
                                skip(
                                    &job.input,
                                    Some(Code::OutUnknownExisting),
                                    "existing output without a manifest record".into(),
                                    &mut summary,
                                );
                                continue;
                            }
                            Some(e) if is_up_to_date(e, job, &encoders) => {
                                summary.up_to_date += 1;
                                rep.emit(
                                    Some(&job.set_key),
                                    Some(&job.input),
                                    EventKind::FileSkipped {
                                        code: None,
                                        reason: "up to date".into(),
                                    },
                                );
                                continue;
                            }
                            Some(_) => {}
                        }
                    }
                }
            }
            // Spec §3.7 (Task 26a): an aliased file records its alias preset and that preset's chain.
            let preset = match &job.suffix_alias {
                Some(a) => plan
                    .registry
                    .alias_chain(&a.preset, plan.target)
                    .ok()
                    .and_then(|c| PresetRef::for_preset(&plan.registry, &a.preset, Some(&c))),
                None => PresetRef::for_preset(&plan.registry, &plan.preset_id, None),
            };
            match preset {
                Some(p) => todo.push((job, p)),
                None => {
                    summary.failed += 1;
                    rep.emit(
                        Some(&job.set_key),
                        Some(&job.input),
                        EventKind::FileFailed {
                            stage: Stage::Resolve,
                            error: "internal error: the resolving preset is not in the registry"
                                .into(),
                        },
                    );
                }
            }
        }

        let limit = if req.mem_budget > 0 {
            req.mem_budget
        } else {
            crate::sysmem::total_ram() / 2
        };
        let budget = Budget {
            limit,
            used: Mutex::new(0),
            cv: Condvar::new(),
        };
        let pool = thread_pool(req.jobs)?;
        let state = Mutex::new(summary);
        // Admission runs on the calling thread (outside the pool) and only spawns admitted files, so no pool thread
        // ever blocks on the budget. A pool thread blocked in `acquire` while its nested band-parallel encode stole
        // another file's task could otherwise hold budget forever (deadlock).
        pool.in_place_scope(|scope| {
            for (job, preset) in &todo {
                if cancel.is_cancelled() {
                    lock(&state).cancelled.push(job.input.clone());
                    continue;
                }
                let est = estimated_bytes(&job.input);
                budget.acquire(est);
                let admitted = Admitted(&budget, est);
                if cancel.is_cancelled() {
                    drop(admitted);
                    lock(&state).cancelled.push(job.input.clone());
                    continue;
                }
                let (state, manifest) = (&state, &manifest);
                scope.spawn(move |_| {
                    let _admitted = admitted;
                    if cancel.is_cancelled() {
                        lock(state).cancelled.push(job.input.clone());
                        return;
                    }
                    run_one(job, req, preset, cancel, rep, manifest, state);
                });
            }
        });
        let mut summary = state.into_inner().unwrap_or_else(|e| e.into_inner());
        summary.cancelled.sort();
        rep.emit(None, None, done_event(&summary));
        Ok(summary)
    }
}

/// Convert one admitted file under `catch_unwind` and record its outcome.
fn run_one(
    job: &ResolvedJob,
    req: &BatchRequest,
    preset: &PresetRef,
    cancel: &CancelToken,
    rep: &Reporter,
    manifest: &Manifest,
    state: &Mutex<BatchSummary>,
) {
    let (set, path) = (Some(job.set_key.as_str()), Some(job.input.as_path()));
    rep.emit(set, path, EventKind::FileStarted);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_file(job, req, preset, cancel, rep)
    }));
    match r {
        Ok(Ok(out)) => {
            if let Err(e) = manifest.append(&out.record) {
                lock(state).failed += 1;
                rep.emit(
                    set,
                    path,
                    EventKind::FileFailed {
                        stage: Stage::Write,
                        error: format!("manifest {}: {e}", manifest.path().display()),
                    },
                );
                return;
            }
            for w in &out.report.warnings {
                rep.emit(set, path, EventKind::Warning(w.clone()));
            }
            {
                let mut s = lock(state);
                s.converted += 1;
                if !out.report.summary.passed {
                    s.validation_failed += 1;
                }
            }
            rep.emit(
                set,
                path,
                EventKind::FileDone {
                    output: slash(&out.output),
                    validation: out.report.summary,
                },
            );
        }
        Ok(Err(FileError::Cancelled)) => lock(state).cancelled.push(job.input.clone()),
        Ok(Err(FileError::Failed(stage, error))) => {
            lock(state).failed += 1;
            rep.emit(set, path, EventKind::FileFailed { stage, error });
        }
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            lock(state).failed += 1;
            rep.emit(
                set,
                path,
                EventKind::FileFailed {
                    stage: Stage::Compress,
                    error: format!("internal error: {msg}"),
                },
            );
        }
    }
}

/// Spec §8: an invalid preset, config, rule or `--set` value, or a setting unavailable for the whole batch, is an
/// error (exit 2) before any work. Profiles are built for every sub-type from the batch preset chain and the
/// configs, then with each rule's and each `--set` override's patch; every suffix alias preset's chain (spec §3.7)
/// must build too.
fn check_up_front(
    req: &BatchRequest,
    registry: &PresetRegistry,
    target: Target,
    chain: &[&PresetFile],
    configs: &[&ConfigFile],
) -> Result<(), EngineError> {
    let usage = EngineError::Usage;
    for o in req.overrides.iter().filter(|o| o.field == "subtype") {
        serde_json::from_value::<Subtype>(o.value.clone())
            .map_err(|_| usage(format!("--set subtype: unknown sub-type {}", o.value)))?;
    }
    let alias_presets: BTreeSet<&str> = registry
        .alias_table()
        .keys()
        .filter_map(|a| registry.alias_for(a).map(|(_, p)| p))
        .collect();
    let mut alias_chains = vec![];
    for pid in alias_presets {
        let c = registry.alias_chain(pid, target).map_err(config_err)?;
        alias_chains.push((pid, c));
    }
    let doc_of = |chain: &[&PresetFile], st: Subtype| {
        let mut doc = Value::Object(Default::default());
        for (_, patch) in PresetRegistry::layers_of(chain, st) {
            merge_json(&mut doc, &patch);
        }
        for c in configs {
            merge_json(&mut doc, &c.defaults);
            if let Some(s) = c.subtypes.get(&st) {
                merge_json(&mut doc, s);
            }
        }
        doc
    };
    let rules_of = |chain: &[&PresetFile]| -> Vec<(String, usize, Value)> {
        let presets = chain.iter().flat_map(|p| {
            p.rules
                .iter()
                .enumerate()
                .map(|(i, r)| (p.id.clone(), i, r.overrides.clone()))
        });
        let cfgs = configs.iter().flat_map(|c| {
            c.rules
                .iter()
                .enumerate()
                .map(|(i, r)| ("config".to_string(), i, r.overrides.clone()))
        });
        presets.chain(cfgs).collect()
    };
    let batch_rules = rules_of(chain);
    for st in Subtype::ALL {
        let doc = doc_of(chain, st);
        finish_profile(doc.clone(), target, st).map_err(|e| {
            EngineError::Config(format!(
                "preset '{}' with the configs, sub-type {}: {e}",
                chain.last().map_or("", |p| p.id.as_str()),
                st.id()
            ))
        })?;
        for (src, i, patch) in &batch_rules {
            let mut d = doc.clone();
            merge_json(&mut d, patch);
            finish_profile(d, target, st).map_err(|e| {
                EngineError::Config(format!("rule {src}#{i}, sub-type {}: {e}", st.id()))
            })?;
        }
        for o in req.overrides.iter().filter(|o| o.field != "subtype") {
            let mut d = doc.clone();
            merge_json(&mut d, &to_patch(&o.field, &o.value));
            finish_profile(d, target, st)
                .map_err(|e| usage(format!("--set {}={}: {e}", o.field, o.value)))?;
        }
        for (pid, c) in &alias_chains {
            let doc = doc_of(c, st);
            finish_profile(doc.clone(), target, st).map_err(|e| {
                EngineError::Config(format!(
                    "preset '{pid}' (suffix alias preset), sub-type {}: {e}",
                    st.id()
                ))
            })?;
            for (src, i, patch) in rules_of(c) {
                let mut d = doc.clone();
                merge_json(&mut d, &patch);
                finish_profile(d, target, st).map_err(|e| {
                    EngineError::Config(format!(
                        "rule {src}#{i} (suffix alias preset '{pid}'), sub-type {}: {e}",
                        st.id()
                    ))
                })?;
            }
        }
    }
    // Spec §8: an unsupported setting for the whole batch exits 2 before work starts.
    for o in req.overrides.iter().filter(|o| o.scope == Scope::All) {
        let bad = match o.field.as_str() {
            "format" => o.value == "bc6h_uf16" && target != Target::Cs,
            "color.tag" => o.value == "srgb" && target != Target::Cs,
            "subtype" => serde_json::from_value::<Subtype>(o.value.clone())
                .is_ok_and(|st| !matches!(availability(target, st), Availability::Available)),
            _ => false,
        };
        if bad {
            return Err(usage(format!(
                "--set {}={} is unavailable for the whole batch under {target:?} (CFG-UNAVAILABLE / TAG-SRGB-VANILLA)",
                o.field, o.value
            )));
        }
    }
    Ok(())
}

fn done_event(s: &BatchSummary) -> EventKind {
    EventKind::BatchDone {
        converted: s.converted,
        up_to_date: s.up_to_date,
        skipped: s.skipped,
        failed: s.failed,
        validation_failed: s.validation_failed,
        cancelled: s.cancelled.iter().map(|p| slash(p)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_precedence() {
        let mut s = BatchSummary::default();
        assert_eq!(s.exit_code(), 0);
        s.up_to_date = 3;
        assert_eq!(s.exit_code(), 0);
        s.validation_failed = 1;
        assert_eq!(s.exit_code(), 4);
        s.cancelled.push("a.png".into());
        assert_eq!(s.exit_code(), 1);
        assert_eq!(EngineError::Usage("x".into()).exit_code(), 2);
        assert_eq!(EngineError::Config("x".into()).exit_code(), 2);
        assert_eq!(EngineError::Fatal("x".into()).exit_code(), 3);
    }

    #[test]
    fn budget_admits_an_oversized_file_alone_and_releases() {
        let b = Budget {
            limit: 10,
            used: Mutex::new(0),
            cv: Condvar::new(),
        };
        b.acquire(100);
        assert_eq!(*lock(&b.used), 100);
        drop(Admitted(&b, 100));
        assert_eq!(*lock(&b.used), 0);
        b.acquire(4);
        b.acquire(6);
        assert_eq!(*lock(&b.used), 10);
    }

    #[test]
    fn target_origin_follows_precedence() {
        let reg = PresetRegistry::builtin().unwrap();
        let chain = reg.chain("vanilla-quality").unwrap();
        let mut req = BatchRequest::new(vec![], "out".into(), Target::Cpm);
        let cfg = |t| ConfigFile {
            target: Some(t),
            ..Default::default()
        };
        let (proj, user) = (cfg(Target::Vanilla), cfg(Target::Cpm));
        assert_eq!(
            resolve_target(&req, Some(&proj), Some(&user), &chain),
            (Target::Cpm, Provenance::User)
        );
        req.target = None;
        assert_eq!(
            resolve_target(&req, Some(&proj), Some(&user), &chain),
            (Target::Vanilla, Provenance::ProjectConfig)
        );
        assert_eq!(
            resolve_target(&req, None, Some(&user), &chain),
            (Target::Cpm, Provenance::UserConfig)
        );
        assert_eq!(
            resolve_target(&req, None, None, &chain),
            (
                Target::Vanilla,
                Provenance::Preset("vanilla-quality".into())
            )
        );
        assert_eq!(
            resolve_target(&req, None, None, &[]),
            (Target::Cs, Provenance::Builtin)
        );
    }
}
