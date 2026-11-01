//! Engine API (spec §2, §5.9, §8): `Engine::plan(BatchRequest) -> Plan`, then `Engine::run(Plan, events, cancel)`.
//!
//! `plan` runs Discover → Group → Load → Analyse → Resolve and reports every decision; nothing is written. Its
//! Load/Analyse work is admitted against `--mem-budget` just like conversion.
//! `run` applies `--on-existing`, then converts the ready files on a rayon pool of `jobs` threads, admitting them
//! by estimated memory against `--mem-budget`. Files that are unresolved, blocked or in error are reported and
//! never converted: the engine does not guess (spec §8, DECISIONS "never auto-decide").
use crate::analyse::analyse_with;
use crate::codes::{Code, Warning};
use crate::config::*;
use crate::discover::{canonicalise_outputs, discover_reporting, InputFile, InputKind};
use crate::events::{EventKind, Reporter, SkipKind, Stage};
use crate::group::{group_sets_with, join_skin_wet, member_split, SuffixSplit, TextureSet};
use crate::image::SourceInfo;
use crate::load::{load_image_meta, LoadError};
use crate::manifest::{
    bc1_model_id, encode_version, output_key, profile_sha256, sha256_file, EncoderVersions,
    Manifest, ManifestEntry, PresetRef, LEGACY_ENCODE_VERSION,
};
use crate::overrides::{project_rel, to_patch, FileScopes, KeyForms, Scope, SetOverride};
use crate::pack;
use crate::pipeline::{process_file, FileError, FileTracker, Processed};
use crate::policy::*;
use crate::presets::*;
use crate::session::{pack_cache_key, AnalysisCache, PackEntry};
use crate::shader::{MeshAlpha, Shader, ShaderPick};
use crate::suggest::{is_pbr_set, SuggestContext};
use encoders_sys::Bc1Model;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// What `run` does when a ready file's output already exists (spec §3.2 `output.on_existing`, §8). It is a batch
/// setting, not a `Profile` field.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum OnExisting {
    /// Re-encode when the manifest shows a different source, profile, engine, encoder or (BC1/BC3) BC1 decoder
    /// model, or a failed validation; keep an output that has no manifest record (`OUT-UNKNOWN-EXISTING`).
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
    /// The user presets folder (`%APPDATA%\sdc\presets`, M6 Track D spec §3): every `*.toml` in it is loaded before
    /// `preset_files`; a bad one is skipped with CFG-PRESET-SKIPPED. `None` (tests) loads nothing.
    pub user_presets: Option<PathBuf>,
    /// Project config; defaults to `sdc.toml` in the first input folder when that exists.
    pub project_config: Option<PathBuf>,
    /// The project config as edited in memory (DECISIONS N4). When `Some`, it is used instead of
    /// `project_config` or the first input folder's `sdc.toml`, and `[files]` keys are relative to the first input.
    pub project_config_inline: Option<ConfigFile>,
    pub overrides: Vec<SetOverride>,
    pub accept: AcceptSuggestions,
    pub allow: BTreeSet<Code>,
    /// Per-file acknowledgements (DECISIONS N2): `[files]` key → codes. Merged with the project config's
    /// `[files.*].allow` (Task 3).
    pub file_allow: BTreeMap<String, BTreeSet<Code>>,
    /// GUI set edits (M3b spec §2.1): applied in order after the automatic grouping. Empty for the CLI.
    pub set_edits: Vec<crate::sets::SetEdit>,
    /// GUI output paths (M3b, DECISIONS P1), relative to `output_root`. Empty for the CLI.
    pub output_paths: BTreeMap<crate::sets::OutputKey, PathBuf>,
    /// GUI (M3b spec §3.3, DECISIONS R7): ids of the sets whose roughness slot (`_rmaos`/`_rfaos`) is one output used
    /// by more than one set, owner and borrowers alike. The engine bakes no normal variance into it and reports no
    /// missing companion for those sets. Empty for the CLI.
    pub shared_roughness: BTreeSet<String>,
    /// GUI (DECISIONS Note R M4-final-1): borrower set id → the id of the set whose `_n` its Normal slot borrows. The
    /// engine bakes the borrower's own roughness member from that `_n`, under every usual bake rule. Empty for the CLI.
    pub borrowed_normal: BTreeMap<String, String>,
    /// GUI (M6, DECISIONS Note S-shader-2): set id → explicit shader. Empty for the CLI; `--set set:<id>.shader`
    /// beats it.
    pub set_shaders: BTreeMap<String, Shader>,
    /// GUI (Note DW): `[files]` keys of the files in a pinned slot of their own set. A set whose diffuse member is
    /// listed gets no Advanced Skin reading from its chosen shader (Note DV1). Empty for the CLI.
    pub pinned_files: BTreeSet<String>,
    /// GUI (M6, DECISIONS Note S-shader-10): `shader::mesh_key` → the shader each scanned lighting shape that uses
    /// `<key>.dds` as its diffuse reports. Empty for the CLI and when no scan exists: the suffix rules decide.
    pub mesh_shaders: BTreeMap<String, Vec<Shader>>,
    /// GUI (M6 Track E, DECISIONS Note U5): `shader::colour_mesh_key` → the NiAlphaProperty of each scanned lighting
    /// shape that uses `<key>.dds` in slot 0. The GUI fills it from the scan database; the CLI sends none (empty: no
    /// evidence, and the alpha question is asked as before).
    pub mesh_alpha: BTreeMap<String, Vec<MeshAlpha>>,
    /// GUI (complex grass design §5): the slot-0 textures of the NIFs that GRAS records of the active load order use,
    /// as `grass::path_key` keys. A listed 1:2 file that passes the detection tests gets a high-confidence
    /// `complex_grass` suggestion. Empty for the CLI, which therefore never auto-tags.
    pub gras_diffuses: BTreeSet<String>,
    /// GUI (PBR grass design §4.2): as `gras_diffuses` with TruePBR shapes kept (`Store::gras_all_diffuse_textures`).
    /// A PBR set with a member listed here auto-picks `pbr_grass`. Empty for the CLI, which never auto-picks it.
    pub gras_all_diffuses: BTreeSet<String>,
    /// GUI (Note BM): ids of the sets with a `[sets.cs_txst]` block. A PBR set listed here auto-picks `pbr_landscape`
    /// under CS. Empty for the CLI.
    pub cs_txst_sets: BTreeSet<String>,
    pub recursive: bool,
    pub on_existing: OnExisting,
    /// Worker threads; 0 = one per logical core.
    pub jobs: usize,
    /// Memory budget in bytes for admitting files; 0 = half of physical RAM.
    pub mem_budget: u64,
    pub bc1_model: Bc1Model,
    /// M5 (DECISIONS Note NV2): an explicit `nvcompress.exe` (`--nvtt`, the GUI's NVTT path). `None` looks on `PATH`,
    /// then in the default install folder. Probed only when some profile uses `encoder.backend = "nvtt"`.
    pub nvtt: Option<PathBuf>,
    /// Also write `<name>.dds.json` next to each output.
    pub sidecar: bool,
    pub dry_run: bool,
    /// GUI (DECISIONS AG91): counters the planner updates while it loads and resolves; nothing is emitted or written.
    pub plan_progress: Option<PlanProgress>,
    /// GUI (v2 artist checks, check 3, DECISIONS Note CL20): measure each eligible texture's detail ratio during the
    /// run. `Off` for the CLI.
    pub low_detail: LowDetailMode,
    /// Track A (DECISIONS Note CN): replace each PBR set's jobs with derived vanilla or complex-material jobs. `None`
    /// (the default) plans as before (C6).
    pub derive: Option<crate::derive::DeriveRequest>,
    /// Track A (Note CN): the folder discovery skips in place of `output_root` (the Also export location, which
    /// holds the PBR, Vanilla and CM folders). `None` skips `output_root` as before.
    pub skip_root: Option<PathBuf>,
}

/// What a run does with a texture whose mip 0 adds little detail over mip 1 (`CHK-LOW-DETAIL`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LowDetailMode {
    /// Nothing is measured.
    #[default]
    Off,
    /// The warning is emitted and the file is written as usual.
    Warn,
    /// `Warn`, and an eligible up-to-date output is loaded and measured again without encoding, so it is flagged
    /// again. The full chain is always written: Hold measures, it never withholds a file (Note FC2, amends CL20).
    Hold,
}

/// How far a plan's per-file pass has got: `total` is the number of files it loads and resolves (duplicates and
/// skipped DDS inputs left out), `done` the files finished so far, cache hits included. One per request, so a
/// cancelled plan still draining never counts into the next one.
#[derive(Debug, Clone, Default)]
pub struct PlanProgress {
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
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
            user_presets: None,
            project_config: None,
            project_config_inline: None,
            overrides: vec![],
            accept: AcceptSuggestions::None,
            allow: BTreeSet::new(),
            file_allow: BTreeMap::new(),
            set_edits: vec![],
            output_paths: BTreeMap::new(),
            shared_roughness: BTreeSet::new(),
            borrowed_normal: BTreeMap::new(),
            set_shaders: BTreeMap::new(),
            pinned_files: BTreeSet::new(),
            mesh_shaders: BTreeMap::new(),
            mesh_alpha: BTreeMap::new(),
            gras_diffuses: BTreeSet::new(),
            gras_all_diffuses: BTreeSet::new(),
            cs_txst_sets: BTreeSet::new(),
            recursive: false,
            on_existing: OnExisting::IfChanged,
            jobs: 0,
            mem_budget: 0,
            bc1_model: Bc1Model::Ideal,
            nvtt: None,
            sidecar: false,
            dry_run: false,
            plan_progress: None,
            low_detail: LowDetailMode::Off,
            derive: None,
            skip_root: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkippedFile {
    pub path: PathBuf,
    /// The output the file would have written (its job key is this relative to the output root).
    pub output: PathBuf,
    /// The key of the texture set the file belongs to.
    pub set: Option<String>,
    pub code: Option<Code>,
    pub reason: String,
}

impl SkippedFile {
    /// What the skip means for a consumer of events: `IN-DDS-SKIPPED` (and `DERIVE-DDS-SOURCE`, a set whose DDS sources are
    /// not re-read, Note CN16), `PACK-CONFLICT`, `GEN-REPLACED` (Note CO), else `IN-DUPLICATE`.
    pub fn kind(&self) -> SkipKind {
        match self.code {
            Some(Code::InDdsSkipped | Code::DeriveDdsSource) => SkipKind::DdsInput,
            Some(Code::PackConflict) => SkipKind::PackConflict,
            Some(Code::DeriveNotConverted) => SkipKind::PackingInput,
            Some(Code::GenReplaced) => SkipKind::GeneratedReplaced,
            _ => SkipKind::Duplicate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FailedFile {
    pub path: PathBuf,
    /// The output the file would have written.
    pub output: PathBuf,
    pub stage: Stage,
    pub error: String,
}

/// Why planning produced no job for a file: its Load failed, or it panicked at the stage it had entered.
enum PlanFailure {
    Load(LoadError),
    Panic(Stage, String),
    /// Assembling a packed image failed other than by a load error (spec §6.4): a source changed size after
    /// pass 1. A load error of a source is a `PlanFailure::Load`, handled like a pass-1 load failure.
    Pack(String),
}

impl PlanFailure {
    /// An assembly failure: a source's load error is a `Load`, anything else a `Pack`.
    fn of_pack(e: pack::assemble::PackError) -> Self {
        match e {
            pack::assemble::PackError::Load(e) => PlanFailure::Load(e),
            e => PlanFailure::Pack(e.to_string()),
        }
    }
}

/// Packing pass 2 and the derive step (spec §5.9, §6.4; Note CN): each item is admitted on the calling thread at
/// `est(item)` bytes, then `work` runs on the pool under `catch_unwind`, so a panic fails that item alone at the stage
/// it had entered (preflight P18). One result per item, in order; `None` = cancelled before its Load started.
fn admit_all<T: Sync>(
    pool: &rayon::ThreadPool,
    budget: &Budget,
    cancel: &CancelToken,
    items: &[T],
    est: impl Fn(&T) -> u64,
    #[cfg_attr(not(feature = "test-hooks"), allow(unused_variables))] output: impl Fn(&T) -> &Path
        + Sync,
    work: impl Fn(&T, Admitted<'_>, &Cell<Stage>) -> Result<ResolvedJob, PlanFailure> + Sync,
) -> Vec<Option<Result<ResolvedJob, PlanFailure>>> {
    let slots: Vec<Mutex<Option<Result<ResolvedJob, PlanFailure>>>> =
        items.iter().map(|_| Mutex::new(None)).collect();
    pool.in_place_scope(|scope| {
        for (item, slot) in items.iter().zip(&slots) {
            if cancel.is_cancelled() {
                continue;
            }
            let est = est(item);
            budget.acquire(est);
            let admitted = Admitted(budget, est);
            let work = &work;
            #[cfg(feature = "test-hooks")]
            let output = &output;
            scope.spawn(move |_| {
                if cancel.is_cancelled() {
                    return;
                }
                let stage = Cell::new(Stage::Load);
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Named by the output, so the pass-1 hook (named by the input) never fires here.
                    #[cfg(feature = "test-hooks")]
                    panic_while_planning_for_test(output(item), Stage::Load);
                    work(item, admitted, &stage)
                }));
                *lock(slot) = Some(r.unwrap_or_else(|panic| {
                    Err(PlanFailure::Panic(
                        stage.get(),
                        format!("internal error: {}", panic_message(&*panic)),
                    ))
                }));
            });
        }
    });
    slots
        .into_iter()
        .map(|s| s.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect()
}

/// The §10 code of a load failure.
fn load_code(e: &LoadError) -> Code {
    match e {
        LoadError::TooLarge { .. } => Code::DimTooLarge,
        LoadError::Decode { .. } => Code::InDecode,
    }
}

pub struct Plan {
    pub request: BatchRequest,
    pub registry: PresetRegistry,
    pub preset_id: String,
    /// The batch target as resolved from `request.target`, the configs or the preset.
    pub target: Target,
    /// Every resolved file, plus one job per packed output (spec §6.4); raw maps consumed by a recipe are not
    /// listed.
    pub jobs: Vec<ResolvedJob>,
    /// Files reported before Resolve: `IN-DUPLICATE` and `IN-DDS-SKIPPED`, and `PACK-CONFLICT` (a pre-packed file
    /// replaced under `packing.prefer = raw`).
    pub skipped: Vec<SkippedFile>,
    /// Files that could not be loaded (`DIM-TOO-LARGE`, `IN-DECODE`), or that panicked while planning (at the
    /// stage they had entered: Load, Analyse or Resolve).
    pub failed: Vec<FailedFile>,
    /// The most estimated bytes admitted at once while loading and analysing (spec §5.9 memory admission).
    pub peak_admitted_bytes: u64,
    /// Files whose Load/Analyse never ran because the batch was cancelled while planning. `run` lists them as
    /// cancelled in `batch_done` (exit 1).
    pub cancelled: Vec<PathBuf>,
    /// M6 (DECISIONS Note S): set key → the set's shader pick.
    pub shaders: BTreeMap<String, ShaderPick>,
    /// The NVTT probe, if any profile on `encoder.backend = "nvtt"` asked for it while planning (DECISIONS Note NV3).
    pub nvtt: Option<crate::nvtt::NvttProbe>,
    /// Note CN23: members of derived or dropped sets whose own output a job of this plan writes (no TA12 fold: a
    /// derived diffuse on its albedo's path, a kept landscape `_p`). That job's event ends them, so `run` emits no
    /// `packed_source` for them.
    pub derive_own: BTreeSet<PathBuf>,
    /// Note DY (review 4 I1): helpers an alpha fill consumed whose target has no job left (failed or cancelled while
    /// planning, or replaced): (helper, the output it filled, set key). `run` ends each with a `packed_source` skip.
    pub fill_ends: Vec<(PathBuf, PathBuf, String)>,
    /// Note DY: every helper of the batch (`helper::report::reports`), by set key then file name.
    pub helpers: Vec<crate::helper::report::HelperReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BatchSummary {
    /// Written outputs, including those whose validation failed.
    pub converted: u32,
    pub up_to_date: u32,
    /// Every `file_skipped` except up-to-date files and raw packing inputs.
    pub skipped: u32,
    /// Of `skipped`, those skipped on purpose: `IN-DDS-SKIPPED` inputs, `PACK-CONFLICT` pre-packed files replaced
    /// under `packing.prefer = raw` and outputs kept by an explicit `--on-existing skip` (low-detail textures are no
    /// longer held, Note FC2). They do not make the batch partial (DECISIONS Notes I, M9 and CL20).
    pub skipped_intentional: u32,
    pub failed: u32,
    /// Written outputs whose validation reported an error (`passed = false`); also counted in `converted`.
    pub validation_failed: u32,
    /// Sorted.
    pub cancelled: Vec<PathBuf>,
}

impl BatchSummary {
    /// Spec §8 precedence for a batch that ran: 1 partial (unintentionally skipped, failed or cancelled
    /// files) > 4 validation failed > 0. Up-to-date files, raw packing inputs and intentional skips
    /// (DECISIONS Note I) do not make a batch partial. (2 and 3 come from
    /// `EngineError`.)
    pub fn exit_code(&self) -> i32 {
        let unintended = self.skipped.saturating_sub(self.skipped_intentional);
        if unintended + self.failed > 0 || !self.cancelled.is_empty() {
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

/// Lock `m`, taking the data of a poisoned lock as it is (engine and session share this).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Memory admission (spec §5.9). Only the scheduling thread acquires; workers release.
///
/// Besides bytes, admission caps the files admitted and not yet finished at `max_files` (the pool size). A pool
/// worker waiting inside one file's band-parallel encode may pick up another admitted file from rayon's queue and
/// run it on the same stack; the cap bounds that nesting by the thread count (review M1).
struct Budget {
    limit: u64,
    max_files: usize,
    /// (bytes, files) admitted and not yet released.
    used: Mutex<(u64, usize)>,
    /// The most bytes ever admitted at once.
    peak: AtomicU64,
    cv: Condvar,
}

impl Budget {
    fn new(limit: u64, max_files: usize) -> Self {
        Budget {
            limit,
            max_files: max_files.max(1),
            used: Mutex::new((0, 0)),
            peak: AtomicU64::new(0),
            cv: Condvar::new(),
        }
    }
    /// Waits until `n` more bytes fit and fewer than `max_files` files are in flight. A file larger than the
    /// whole budget is admitted alone.
    fn acquire(&self, n: u64) {
        let mut used = lock(&self.used);
        while used.1 >= self.max_files || (used.0 > 0 && used.0.saturating_add(n) > self.limit) {
            used = self.cv.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        used.0 += n;
        used.1 += 1;
        self.peak.fetch_max(used.0, Ordering::Relaxed);
    }
    fn release(&self, n: u64) {
        {
            let mut used = lock(&self.used);
            used.0 -= n;
            used.1 -= 1;
        }
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
/// A DDS input (`image` cannot read its size) also holds its file data while mip 0 decodes (`load::load_dds`).
fn estimated_bytes(path: &Path) -> u64 {
    let Some((w, h)) = input_dims(path) else {
        return 0;
    };
    let file = if is_dds(path) {
        std::fs::metadata(path).map_or(0, |m| m.len())
    } else {
        0
    };
    input_bytes(w, h) + file
}

fn is_dds(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
}

/// The input's size: a DDS from its header (`load::dds_header_size`), anything else by content
/// (`load::image_size`, MAX_DIM-checked; a source refused here is refused at load too, so it holds nothing).
/// `None` when the size cannot be read.
fn input_dims(path: &Path) -> Option<(u32, u32)> {
    if is_dds(path) {
        let ((w, h), cube) = crate::load::dds_header_facts(path)?;
        // Clamped: a larger DDS is refused at load, and a hostile header must not overflow `input_bytes`.
        let dim = |v: u32| v.min(crate::load::MAX_DIM);
        // A cube DDS loads as a 6w x h strip (DECISIONS DS7), so that is what it holds.
        return Some((dim(w) * if cube { 6 } else { 1 }, dim(h)));
    }
    crate::load::image_size(path).ok()
}

/// [`estimated_bytes`] of a `w`×`h` input.
fn input_bytes(w: u32, h: u32) -> u64 {
    u64::from(w) * u64::from(h) * 16 * 134 / 100 * 3
}

/// [`estimated_bytes`] of the job's input, plus what its variance bake holds (DECISIONS R4; the M4 carry "the
/// summed-area table counts against `--mem-budget`", a length pyramid since R5), plus (S-mips-11) 8 B/texel for a
/// non-box filter's intermediate and 32 B/texel for a resize, both on the input's size, plus (Note BB) the active
/// RNM layers' [`crate::rnm::RnmJob::admission_bytes`] at the resized size and, for complex grass, one more float
/// chain (both half chains and the stacked chain are live together in `grass::build`).
fn job_bytes(job: &ResolvedJob) -> u64 {
    let p = &job.profile;
    let extra = input_dims(&job.input).map_or(0, |(w, h)| {
        let texels = u64::from(w) * u64::from(h);
        let filter = if p.mips.filter != MipFilter::Box {
            8
        } else {
            0
        };
        let resize = if crate::resize::target_dims((w, h), p.resize) != (w, h) {
            32
        } else {
            0
        };
        let grass = if p.subtype == Subtype::ComplexGrass {
            texels * 16 * 134 / 100
        } else {
            0
        };
        let layers = job.active_rnm().map_or(0, |r| {
            r.admission_bytes(&p.rnm, crate::resize::target_dims((w, h), p.resize))
        });
        // Note CN13: a derived job also holds the colour maps it reads, its scalars (and `_curves`/cavity) image and
        // the recipe's output, twice the input for the grass stack.
        let derived = job.derive.as_ref().map_or(0, |dj| {
            use crate::derive::DeriveKind::*;
            let colour = match dj.kind {
                VanillaDiffuse | CmDiffuse | LegacyN => 1,
                CmM | EnvM => 0,
                GrassStack => 2,
            };
            let detail = u64::from(dj.curves.is_some() || dj.cavity.is_some());
            let output = if dj.kind == GrassStack { 2 } else { 1 };
            texels * 16 * (colour + 1 + detail + output)
        });
        texels * (filter + resize) + grass + layers + derived
    });
    // DECISIONS DS8: a cube holds every source plus three float cubes of its face size (base, blur copy, mip chain).
    let cube = job.cube.as_ref().map_or(0, |c| {
        let others: u64 = c
            .sources
            .iter()
            .skip(1)
            .map(|s| estimated_bytes(&s.path))
            .sum();
        let dims = input_dims(&job.input);
        let face = dims.map_or(0, |src| {
            u64::from(crate::cube::layout::face_size(p, c.layout, src))
        });
        // DS19: a single photo also holds its fake 2w x w float panorama (`import_dir::photo_panorama`).
        let panorama = match (c.layout, dims) {
            (CubeLayout::Photo, Some((w, _))) => 2 * u64::from(w) * u64::from(w) * 16,
            _ => 0,
        };
        others + 6 * face * face * 16 * 3 + panorama
    });
    estimated_bytes(&job.input)
        + extra
        + cube
        // Note DY: `apply_fill` holds the helper's decoded image beside the target's.
        + job.fill.as_ref().map_or(0, |f| estimated_bytes(&f.source.path))
        + job
            .bake
            .as_ref()
            .map_or(0, crate::bake::BakeJob::estimated_bytes)
}

/// The admission limit: `--mem-budget`, else half of physical memory (spec §5.9).
fn budget_limit(req: &BatchRequest) -> u64 {
    if req.mem_budget > 0 {
        req.mem_budget
    } else {
        crate::sysmem::total_ram() / 2
    }
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

/// The skip code of an unresolved job: its `SUG-LOW-CONFIDENCE` warning.
fn unresolved_code(job: &ResolvedJob) -> Code {
    job.warnings
        .iter()
        .map(|w| w.code)
        .find(|c| *c == Code::SugLowConfidence)
        .unwrap_or(Code::SugLowConfidence)
}

/// (SHA-256, bake identity, RNM role, derive identity, fill identity) of one recorded source.
type SourceKey = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Every recorded source of `job` now, in record order: the pack sources, then the baked `_n` (DECISIONS R12), then
/// the RNM inputs (M6 Track F2), as `SourceKey`s. Empty for an ordinary file that does not bake; `None` when a
/// source cannot be hashed (then the output is not up to date).
fn current_sources(job: &ResolvedJob) -> Option<Vec<SourceKey>> {
    let mut v = vec![];
    if let Some(pk) = &job.pack {
        for s in &pk.sources {
            v.push((
                sha256_file(&s.path).ok()?,
                None,
                None,
                None,
                pack::source_fill(pk, s),
            ));
        }
    }
    // Note CN12: a derived job's sources, each with the job's identity, so a changed setting re-encodes.
    if let Some(dj) = &job.derive {
        let identity = dj.identity_sha256();
        for s in &dj.sources {
            v.push((
                sha256_file(&s.path).ok()?,
                None,
                None,
                Some(identity.clone()),
                None,
            ));
        }
    }
    // DECISIONS DS8: a six-file cube's faces in face order; a one-file cube is `source_sha256` alone.
    for s in job
        .cube
        .iter()
        .filter(|c| c.sources.len() > 1)
        .flat_map(|c| &c.sources)
    {
        v.push((sha256_file(&s.path).ok()?, None, None, None, None));
    }
    if let Some(b) = &job.bake {
        v.push((
            sha256_file(&b.normal).ok()?,
            Some(b.identity_sha256()),
            None,
            None,
            None,
        ));
        if let Some(r) = &b.rnm {
            for (f, role) in r.inputs(&b.normal_profile.rnm) {
                v.push((
                    sha256_file(&f.path).ok()?,
                    None,
                    Some(format!("bake.{role}")),
                    None,
                    None,
                ));
            }
        }
    }
    if let Some(r) = job.active_rnm() {
        for (f, role) in r.inputs(&job.profile.rnm) {
            v.push((sha256_file(&f.path).ok()?, None, Some(role), None, None));
        }
    }
    // Note DY: the helper that fills the alpha, last, as `process_file` records it.
    if let Some(f) = &job.fill {
        v.push((
            sha256_file(&f.source.path).ok()?,
            None,
            None,
            None,
            Some(f.identity()),
        ));
    }
    Some(v)
}

/// A packed job's sources fill the same roles, inverted the same way, as the record's (DECISIONS N7). A schema-1
/// record has no roles, so it never matches and the output re-encodes once. Ordinary jobs always match.
fn pack_roles_match(e: &ManifestEntry, job: &ResolvedJob) -> bool {
    match &job.pack {
        None => true,
        Some(pk) => e
            .sources
            .iter()
            .filter(|r| r.bake.is_none() && r.rnm.is_none())
            .map(|r| (r.role, r.invert))
            .eq(pk
                .sources
                .iter()
                .map(|s| (Some(s.role), pack::invert_of(pk, &s.path)))),
    }
}

/// `--sidecar` on an up-to-date output (audit 15, DECISIONS Note Y2): write the stored record when the sidecar is
/// missing, or when it is sdc's record of an earlier output (a failed write left it, review m10 / Note BV4). A file
/// sdc did not write is left alone (BV3). `false` when the record cannot be written, so the file re-encodes and the
/// encode reports the error.
fn sidecar_present(output: &Path, e: &ManifestEntry) -> bool {
    let side = output.with_extension("dds.json");
    match std::fs::read(&side) {
        Ok(b) => {
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else {
                return true;
            };
            let stale_ours =
                v != e.raw && v["output"] == e.raw["output"] && v.get("profile_sha256").is_some();
            !stale_ours
                || serde_json::to_string_pretty(&e.raw)
                    .is_ok_and(|t| crate::dds::write_atomic(&side, t.as_bytes()).is_ok())
        }
        Err(e2) if e2.kind() != std::io::ErrorKind::NotFound => true,
        Err(_) => serde_json::to_string_pretty(&e.raw)
            .is_ok_and(|t| crate::dds::write_atomic(&side, t.as_bytes()).is_ok()),
    }
}

/// Whether the record's encode version equals the type's version now. A record without one (written by 1.0.0)
/// counts as `LEGACY_ENCODE_VERSION`, so it is current only when the type's version is still that.
pub(crate) fn encode_current(e: &ManifestEntry, now: &str) -> bool {
    match &e.encode_version {
        Some(v) => v == now,
        None => e.engine_version == LEGACY_ENCODE_VERSION && now == LEGACY_ENCODE_VERSION,
    }
}

/// Whether a manifest entry proves the existing output is current (spec §8 `if-changed`, carry 13): same source
/// (the bytes of every source and the channel each feeds, for a packed output), profile, the sub-type's encode
/// version (DECISIONS Note DM; a record from 1.0.0 without one counts as 1.0.0) and encoders, a comparable record (known schema, encoders and preset present) and a passing validation, so a
/// failed output is re-encoded and reported again rather than counted as up to date
/// (controller ruling O2). BC1 and BC3 outputs also need the same batch BC1 decoder model: rgbcx encodes their
/// colour blocks against it (BC3's colour half is a BC1 block), and validation decodes BC1 with it (review I2).
pub(crate) fn is_up_to_date(
    e: &ManifestEntry,
    job: &ResolvedJob,
    encoders: &EncoderVersions,
    bc1_model: Bc1Model,
    nvtt: Option<&str>,
) -> bool {
    let Some(rec) = e.encoders.as_ref() else {
        return false;
    };
    // The rgbcx patch only changes BC4 blocks (BC3 alpha, BC4, BC5), so other formats ignore it.
    let rgbcx = matches!(job.profile.format, Format::Bc3 | Format::Bc4 | Format::Bc5);
    if e.is_newer_schema()
        || e.preset.is_none()
        || (&rec.bc7enc_rdo, &rec.ispctc, &rec.ispc, &rec.bc7e_patch)
            != (
                &encoders.bc7enc_rdo,
                &encoders.ispctc,
                &encoders.ispc,
                &encoders.bc7e_patch,
            )
        || (rgbcx && rec.rgbcx_patch != encoders.rgbcx_patch)
        // NV10: an NVTT output follows the NVTT version found now; a built-in one ignores it.
        || (job.profile.encoder.backend == Backend::Nvtt && rec.nvtt.as_deref() != nvtt)
    {
        return false;
    }
    // Note Y1: metadata only, and before any hashing; a legacy record (no stamp) is stale once.
    if e.output_stamp.is_none() || SourceStamp::of(&job.output) != e.output_stamp {
        return false;
    }
    if matches!(job.profile.format, Format::Bc1 | Format::Bc3)
        && e.bc1_model.as_deref() != Some(bc1_model_id(bc1_model))
    {
        return false;
    }
    let passed = e
        .raw
        .get("validation")
        .and_then(|v| v.get("passed"))
        .and_then(Value::as_bool)
        == Some(true);
    passed
        && encode_current(e, encode_version(job.profile.subtype))
        && current_sources(job).is_some_and(|now| {
            now.into_iter().eq(e.sources.iter().map(|r| {
                (
                    r.sha256.clone(),
                    r.bake.clone(),
                    r.rnm.clone(),
                    r.derive.clone(),
                    r.fill.clone(),
                )
            }))
        })
        && pack_roles_match(e, job)
        && profile_sha256(&job.profile).is_ok_and(|h| h == e.profile_sha256)
        && sha256_file(&job.input).is_ok_and(|h| h == e.source_sha256)
}

pub struct Engine {
    registry: PresetRegistry,
}

/// The folder `[files]` keys are relative to (DECISIONS N2, N4).
pub fn project_root(req: &BatchRequest) -> Option<PathBuf> {
    let first_dir = || req.inputs.first().filter(|p| p.is_dir()).cloned();
    if req.project_config_inline.is_some() {
        return first_dir();
    }
    match &req.project_config {
        // `--config sdc.toml` has an empty parent: that is the current directory.
        Some(p) => p.parent().map(crate::overrides::absolute_or_same),
        None => first_dir().filter(|d| d.join("sdc.toml").exists()),
    }
}

/// The project config: the inline one, `--config`, or the first input folder's `sdc.toml`.
fn load_project_config(req: &BatchRequest) -> Result<Option<ConfigFile>, EngineError> {
    if let Some(c) = &req.project_config_inline {
        return Ok(Some(c.clone()));
    }
    let project_path = req.project_config.clone().or_else(|| {
        req.inputs
            .first()
            .filter(|p| p.is_dir())
            .map(|p| p.join("sdc.toml"))
            .filter(|p| p.exists())
    });
    project_path
        .as_deref()
        .map(ConfigFile::load)
        .transpose()
        .map_err(config_err)
}

/// What [`Engine::preflight`]'s checks resolve, kept for planning.
struct Setup {
    registry: PresetRegistry,
    user_config: Option<ConfigFile>,
    project_config: Option<ConfigFile>,
    target: Target,
    target_src: Provenance,
    preset_id: String,
    preset_skips: Vec<Warning>,
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

    /// The project config's `[export]`, else the default. Parses only: no `check()`, so a bad `[export.legacy]`
    /// value cannot break a plain export (O2). `run_also` checks it before anything else.
    pub fn export_config(&self, req: &BatchRequest) -> Result<ExportConfig, EngineError> {
        Ok(load_project_config(req)?
            .and_then(|c| c.export)
            .unwrap_or_default())
    }

    /// Spec §8 up-front checks without reading any input: load the preset files and the configs (the user
    /// config, `--config` or the first input folder's `sdc.toml`), resolve the target and preset, and build every
    /// profile with each rule and `--set` value. An invalid one is an error (exit 2). `plan` runs the same checks
    /// first; `inspect` calls this on its own so a DDS-only or empty input is held to them too (Codex-CLI 2).
    /// Returns the CFG-PRESET-SKIPPED warnings for the caller to report when it does not go on to `plan`.
    pub fn preflight(&self, req: &BatchRequest) -> Result<Vec<Warning>, EngineError> {
        self.setup(req).map(|s| s.preset_skips)
    }

    fn setup(&self, req: &BatchRequest) -> Result<Setup, EngineError> {
        let mut registry = self.registry.clone();
        // Spec §3: the user folder first (bad files skipped), then `--preset-file` (a bad one is an error).
        let preset_skips = req
            .user_presets
            .as_deref()
            .map(|d| registry.add_dir(d))
            .unwrap_or_default();
        for f in &req.preset_files {
            registry.add_file(f).map_err(config_err)?;
        }
        let user_path = req.user_config.as_deref().filter(|p| p.exists());
        let user_config = user_path
            .map(ConfigFile::load)
            .transpose()
            .map_err(config_err)?;
        if let (Some(p), Some(c)) = (user_path, &user_config) {
            if !c.files.is_empty() {
                return Err(EngineError::Config(format!(
                    "[files] is only valid in a project sdc.toml, not in {}",
                    p.display()
                )));
            }
            if c.export.is_some() {
                return Err(EngineError::Config(format!(
                    "[export] is only valid in a project sdc.toml, not in {}",
                    p.display()
                )));
            }
            if !c.inputs.is_empty() {
                return Err(EngineError::Config(format!(
                    "[inputs] is only valid in a project sdc.toml, not in {}",
                    p.display()
                )));
            }
        }
        let project_config = load_project_config(req)?;

        // Target and preset. An explicitly chosen preset may state the target; otherwise the target picks the
        // default preset.
        let chosen = req
            .preset
            .clone()
            .or_else(|| project_config.as_ref().and_then(|c| c.preset.clone()))
            .or_else(|| user_config.as_ref().and_then(|c| c.preset.clone()));
        let (target, target_src, preset_id) = {
            let chosen_chain = match &chosen {
                Some(id) => registry.chain(id).map_err(preset_err)?,
                None => vec![],
            };
            let (target, target_src) = resolve_target(
                req,
                project_config.as_ref(),
                user_config.as_ref(),
                &chosen_chain,
            );
            let preset_id = chosen.unwrap_or_else(|| default_preset_for(target).to_string());
            let chain = registry.chain(&preset_id).map_err(preset_err)?;
            let configs: Vec<&ConfigFile> =
                user_config.iter().chain(project_config.iter()).collect();
            check_up_front(req, &registry, target, &chain, &configs)?;
            (target, target_src, preset_id)
        };
        Ok(Setup {
            registry,
            user_config,
            project_config,
            target,
            target_src,
            preset_id,
            preset_skips,
        })
    }

    /// Discover, group, load, analyse and resolve every input, emitting `batch_started`, `file_discovered`,
    /// `set_resolved`, `file_planned` and every warning. Presets, configs, rules and `--set` values are
    /// validated first ([`Engine::preflight`]): any invalid one is an error (exit 2) before any file is read.
    /// Once `cancel` is set, no further file is loaded; those files are returned in `Plan::cancelled`. Each file's
    /// Load, Analyse and Resolve run under `catch_unwind`: a panic fails that file alone (`Plan::failed`).
    pub fn plan(
        &self,
        req: BatchRequest,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<Plan, EngineError> {
        self.plan_cached(req, rep, cancel, &AnalysisCache::default())
    }

    /// [`Engine::plan`] reusing `cache`: a file whose size and modification time are unchanged is not reloaded, and
    /// neither is a packed image whose recipe and sources are unchanged (M3a spec §4.4).
    pub fn plan_cached(
        &self,
        req: BatchRequest,
        rep: &Reporter,
        cancel: &CancelToken,
        cache: &AnalysisCache,
    ) -> Result<Plan, EngineError> {
        let Setup {
            registry,
            user_config,
            project_config,
            target,
            target_src,
            preset_id,
            preset_skips,
        } = self.setup(&req)?;

        for i in &req.inputs {
            if !i.exists() {
                return Err(EngineError::Usage(format!(
                    "input not found: {}",
                    i.display()
                )));
            }
        }
        let (mut files, unreadable) = discover_reporting(
            &req.inputs,
            &req.output_root,
            req.recursive,
            req.skip_root.as_deref(),
        )
        .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
        // F3b: `[inputs]` aliases rename files before anything reads their names.
        let read_as_warnings = match (&project_config, project_root(&req)) {
            (Some(c), Some(root)) if !c.inputs.is_empty() => {
                crate::discover::apply_read_as(&mut files, &root, &req.output_root, &c.inputs)
            }
            _ => vec![],
        };
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
        for w in preset_skips.into_iter().chain(read_as_warnings) {
            rep.emit(None, None, EventKind::Warning(w));
        }
        for u in unreadable {
            rep.emit(
                None,
                None,
                EventKind::Warning(Warning::new(Code::InUnreadable, format!("skipped: {u}"))),
            );
        }
        let key = |out: &Path| output_key(&req.output_root, out);
        // M3b planning sequence (spec §2.1, DECISIONS P1). It is load-bearing:
        // 1. `discover`, then `[inputs]` aliases (`apply_read_as`, F3b), then `canonicalise_outputs` (above).
        // 2. `group_sets_with` gives the automatic sets; its duplicate-output groups are ignored from now on. On cs,
        //    `join_skin_wet` then moves a skin `_wet` member into its Advanced skin set.
        // 3. `apply_set_edits`. Set keys are fixed from here on: output overrides never change a key.
        // 4. `apply_output_paths` (Task 3).
        // 5. `duplicate_outputs_with` on the final paths, so two sets writing one file are caught after paths change.
        // 6. Predicted packing suffixes (they read the explicit shaders of 6a, Note AZ), the skip/work loop, then pass
        //    1 (Load, Analyse, Resolve with no shaders).
        // 6a. Explicit shader choices (`explicit_shaders`): `BatchRequest::set_shaders`, then `--set set:<id>.shader`.
        // 6b. One `ShaderPick` per set (`shader::pick_for_set`) from the pass-1 sub-types; `ctx.shaders` holds them.
        // 6c. Members of a set whose shader has member defaults are resolved again with the picks; then packing.
        let scope_root =
            project_root(&req).or_else(|| req.inputs.first().filter(|p| p.is_dir()).cloned());
        let (mut sets, _) = group_sets_with(&files, registry.alias_table());
        let (file_overrides, mut file_allow) = match &project_config {
            Some(c) => c.file_entries().map_err(config_err)?,
            None => (vec![], BTreeMap::new()),
        };
        for (k, codes) in &req.file_allow {
            file_allow
                .entry(k.clone())
                .or_default()
                .extend(codes.iter().copied());
        }
        // Note DW: the pinned files that are a set's diffuse member (no suffix, or `_d`).
        let skin_pinned = match &scope_root {
            Some(root) if !req.pinned_files.is_empty() => {
                let index = crate::overrides::KeyIndex::new(root, &files);
                req.pinned_files
                    .iter()
                    .flat_map(|k| index.hits(k).iter().copied())
                    .filter(|&i| {
                        let stem = files[i].rel.file_stem().unwrap_or_default();
                        matches!(
                            crate::group::split_suffix_with(
                                &stem.to_string_lossy(),
                                registry.alias_table()
                            )
                            .suffix
                            .as_deref(),
                            None | Some("d")
                        )
                    })
                    .collect()
            }
            _ => BTreeSet::new(),
        };
        let mut file_scopes = FileScopes {
            root: scope_root.clone(),
            overrides: file_overrides,
            allow: file_allow,
            skin_pinned,
            ..FileScopes::default()
        };
        file_scopes.index(&files);
        let nvtt = crate::nvtt::NvttCell::new(req.nvtt.clone());
        let ctx = ResolveCtx {
            target,
            registry: &registry,
            preset: &preset_id,
            user_config: user_config.as_ref(),
            project_config: project_config.as_ref(),
            overrides: &req.overrides,
            accept: req.accept,
            allow: &req.allow,
            files: &file_scopes,
            shaders: &NO_SHADERS,
            mesh_alpha: &req.mesh_alpha,
            nvtt: Some(&nvtt),
        };
        // Trace-debug 2026-10-16 #4: a raw map whose sub-type was requested is an ordinary file (spec §4.4), as
        // `plan_packing` treats it, so its role leaves the set's suffixes (unless another raw member has it) and
        // Advanced skin detection and the `auto` prediction below follow the recipe actually built.
        // shortcut: rule-named sub-types are not seen here (rules need the analysis); revisit if a rule names one.
        // Note DY: each image's hand-set `helper.role`. `none` makes a raw map an ordinary file, like a requested
        // sub-type; any other hand role wins over a requested sub-type (ruling 13).
        let hand: Vec<Option<crate::helper::HelperRole>> = files
            .iter()
            .map(|f| {
                (f.kind == InputKind::Image)
                    .then(|| crate::policy::helper_requested(&ctx, f).role)
                    .flatten()
            })
            .collect();
        let typed: BTreeSet<usize> = (0..files.len())
            .filter(|&i| {
                pack::plan::raw_role(&files[i]).is_some()
                    && match hand[i] {
                        Some(r) => r == crate::helper::HelperRole::NotHelper,
                        None => crate::policy::subtype_requested(&ctx, &files[i]),
                    }
            })
            .collect();
        let drop_typed_roles = |sets: &mut Vec<TextureSet>| {
            for set in sets.iter_mut() {
                let role = |i: usize| pack::plan::raw_role(&files[i]).map(|r| r.id());
                let (typed_m, raw_m): (Vec<usize>, Vec<usize>) =
                    set.members.iter().partition(|i| typed.contains(i));
                for r in typed_m.into_iter().filter_map(role) {
                    if !raw_m.iter().any(|&j| role(j).as_ref() == Some(&r)) {
                        set.suffixes.remove(&r);
                    }
                }
            }
        };
        drop_typed_roles(&mut sets);
        // Note DY: a hand-set helper with no recognised suffix (`x_mask`) joins set `x` when it exists.
        crate::group::join_helpers(&mut sets, &files, registry.alias_table(), |i| {
            hand[i].is_some_and(|r| r != crate::helper::HelperRole::NotHelper)
        });
        // Final review I3 (DECISIONS Note R): `x_wet` joins set `x` as its skin `_wet` member only on cs and only
        // when `x` is Advanced skin. Before set edits, so they see the joined set like any automatic one.
        // shortcut: a set skin only by its chosen shader (Note DV7) is not seen here; move the explicit shaders up
        // if such a project needs its `_wet` joined.
        if target == Target::Cs {
            join_skin_wet(&mut sets, &files, registry.alias_table(), |s| {
                s.members
                    .first()
                    .is_some_and(|&i| pack::plan::set_is_advanced_skin(&ctx, s, &files[i]))
            });
        }
        let stale = crate::sets::apply_set_edits(
            &mut sets,
            &files,
            scope_root.as_deref(),
            &req.output_root,
            registry.alias_table(),
            &req.set_edits,
        );
        // A touched set's suffixes were rebuilt from its members' names.
        drop_typed_roles(&mut sets);
        for s in &stale {
            rep.emit(
                None,
                None,
                EventKind::Warning(
                    Warning::new(
                        Code::SetEditStale,
                        format!("set edit skipped: {}", s.reason),
                    )
                    .with_data(serde_json::json!({ "edit": s.edit, "reason": s.reason })),
                ),
            );
        }
        crate::sets::apply_output_paths(
            &mut files,
            scope_root.as_deref(),
            &req.output_root,
            &req.output_paths,
        );
        // Read by 3b too (CO102): a DDS at a packed output's explicit path is that output's earlier export.
        let packed_outputs = crate::sets::packed_outputs(&req.output_root, &req.output_paths);
        // 3b, Note CO (the M9 fold): a generated member loses to a real map for its slot before duplicates and
        // pack planning see it, since a generated `_p` and a real `_p` share one output.
        let mut gen_replaced: Vec<SkippedFile> = vec![];
        let mut losers: BTreeSet<PathBuf> = BTreeSet::new();
        for (g, r) in crate::group::generated_losers(
            &sets,
            &files,
            registry.alias_table(),
            target,
            &packed_outputs,
        ) {
            let set = sets
                .iter_mut()
                .find(|s| s.members.contains(&g))
                .expect("a loser is a set member");
            set.members.retain(|&i| i != g);
            let suffix_of = |i: usize| {
                let stem = files[i]
                    .rel
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                crate::group::split_suffix_with(&stem, registry.alias_table()).suffix
            };
            if let Some(sfx) = suffix_of(g) {
                if !set
                    .members
                    .iter()
                    .any(|&i| suffix_of(i).as_ref() == Some(&sfx))
                {
                    set.suffixes.remove(&sfx);
                }
            }
            let name = |i: usize| {
                files[i]
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            };
            let dds = if files[r].kind == InputKind::Dds {
                " (a DDS: not written to the output unless DDS inputs are re-encoded)"
            } else {
                ""
            };
            losers.insert(files[g].path.clone());
            gen_replaced.push(SkippedFile {
                path: files[g].path.clone(),
                output: files[g].output.clone(),
                set: Some(set.key.clone()),
                code: Some(Code::GenReplaced),
                reason: format!("generated {} replaced by {}{dds}", name(g), name(r)),
            });
        }
        for f in &files {
            rep.emit_job(
                Some(&key(&f.output)),
                None,
                Some(&f.path),
                EventKind::FileDiscovered {
                    output: slash(&f.output),
                },
            );
        }
        let reencode =
            |f: &InputFile| f.kind == InputKind::Dds && crate::policy::reencode_requested(&ctx, f);
        let dups = crate::group::duplicate_outputs_with(&files, |f| {
            !losers.contains(&f.path) && (f.kind != InputKind::Dds || reencode(f))
        });

        // Spec §6.4 "Set membership", DECISIONS Note M7: a recipe's output joins its set before anything resolves.
        // Note AZ: a set explicitly given complex material predicts the complex `_m` (the auto pick comes later and
        // needs a member already typed complex_material, so it never adds a suffix of its own).
        let (explicit, stale_shaders) = explicit_shaders(&req, &sets);
        for set in &mut sets {
            let roles: BTreeSet<RawMap> = set
                .members
                .iter()
                .filter(|i| !typed.contains(i))
                .filter_map(|&i| match hand[i] {
                    Some(crate::helper::HelperRole::Raw(r)) => Some(r),
                    Some(_) => None,
                    None => pack::plan::raw_role(&files[i]),
                })
                .collect();
            let complex = explicit.get(&set.key) == Some(&Shader::ComplexMaterial);
            for s in pack::predicted_suffixes(target, complex, &roles) {
                set.suffixes.insert(s.to_string());
            }
        }
        let set_of: BTreeMap<usize, &str> = sets
            .iter()
            .flat_map(|s| s.members.iter().map(|&i| (i, s.key.as_str())))
            .collect();
        let dup_idx: BTreeSet<usize> = dups.iter().flatten().copied().collect();
        let mut skipped: Vec<SkippedFile> = dup_idx
            .iter()
            .map(|&i| SkippedFile {
                path: files[i].path.clone(),
                output: files[i].output.clone(),
                set: set_of.get(&i).map(|k| k.to_string()),
                code: Some(Code::InDuplicate),
                reason: "two inputs map to one output; exclude one".into(),
            })
            .collect();
        skipped.extend(gen_replaced);
        let mut self_overwrite: Vec<FailedFile> = vec![];
        let mut work: Vec<(&TextureSet, &InputFile)> = vec![];
        for set in &sets {
            for &i in &set.members {
                if dup_idx.contains(&i) {
                    continue;
                }
                if files[i].kind == InputKind::Dds {
                    if !reencode(&files[i]) {
                        skipped.push(SkippedFile {
                            path: files[i].path.clone(),
                            output: files[i].output.clone(),
                            set: Some(set.key.clone()),
                            code: Some(Code::InDdsSkipped),
                            reason: "DDS inputs are not re-encoded by default".into(),
                        });
                        continue;
                    }
                    // DECISIONS S6: never write over the source DDS (output root == input folder).
                    if crate::dds::same_file(&files[i].path, &files[i].output) {
                        self_overwrite.push(FailedFile {
                            path: files[i].path.clone(),
                            output: files[i].output.clone(),
                            stage: Stage::Load,
                            error: "re-encoding would overwrite the source DDS: choose another output folder".into(),
                        });
                        continue;
                    }
                }
                work.push((set, &files[i]));
            }
        }

        // Load and Analyse in parallel under the same memory admission as `run` (spec §5.9, Codex-B I1): the calling
        // thread admits each file's estimated bytes before spawning it, and the bytes are released as soon as its
        // analysis is done (or it unwinds). Only the `Analysis` is kept.
        // Shared by pass 1 and the 6c re-resolve; each call site builds `FileFacts` from its own split (it borrows it).
        let split_for = |(set, f): (&TextureSet, &InputFile)| -> SuffixSplit {
            let stem = f
                .rel
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            member_split(&stem, set, registry.alias_table())
        };
        let gras = &req.gras_diffuses;
        // A sub-type a `[files]` entry or `--set` gave the file picks its hints; else the name's suffix does.
        let hints_of = |ctx: &ResolveCtx, f: &InputFile, split: &SuffixSplit| {
            match crate::policy::subtype_override(ctx, f) {
                Some(st) => crate::checks::hints_for_subtype(st, cache.checks),
                None => crate::checks::hints_for(split.suffix.as_deref(), cache.checks),
            }
        };
        let resolve_one =
            |&(set, f): &(&TextureSet, &InputFile),
             admitted: Admitted<'_>,
             stage: &Cell<Stage>|
             -> Result<(ResolvedJob, crate::analyse::Analysis, SourceInfo), LoadError> {
                let enter = |st: Stage| {
                    stage.set(st);
                    #[cfg(feature = "test-hooks")]
                    panic_while_planning_for_test(&f.path, st);
                };
                enter(Stage::Load);
                let split = split_for((set, f));
                // Taken before the read, so a change made while it is loaded is seen as a change too.
                let source = SourceStamp::of(&f.path);
                let hints = hints_of(&ctx, f, &split);
                let (a, info) = match source.and_then(|s| cache.file(&f.path, s, hints)) {
                    Some(hit) => {
                        drop(admitted);
                        hit
                    }
                    None => {
                        let (img, info, meta) = load_image_meta(&f.path)?;
                        cache.count_loads(1);
                        enter(Stage::Analyse);
                        let mut a = analyse_with(&img, &info, hints);
                        a.icc_profile = meta.icc_profile;
                        a.dds_dxgi = meta.dds_dxgi;
                        a.dds_cube = meta.dds_cube;
                        if let Some(s) = source {
                            cache.put_file(
                                &f.path,
                                s,
                                hints,
                                a.clone(),
                                info,
                                cache.make_thumb(&img, &info),
                            );
                        }
                        drop(img);
                        drop(admitted);
                        (a, info)
                    }
                };
                enter(Stage::Resolve);
                let facts = FileFacts {
                    input: f,
                    suffix: split.suffix.as_deref(),
                    alias: split.alias.as_deref(),
                    set,
                    analysis: &a,
                    info: &info,
                };
                let gras_diffuse = gras.contains(&crate::shader::gras_key(&f.game_path, &f.output));
                let mut job = resolve_file_with(&ctx, &facts, gras_diffuse);
                job.provenance.insert("target".into(), target_src.clone());
                job.source = source;
                Ok((job, a, info))
            };
        let pool = thread_pool(req.jobs)?;
        let budget = Budget::new(budget_limit(&req), pool.current_num_threads());
        // One slot per work item, so results keep the input order. A slot left empty is a file that was cancelled
        // before its Load started.
        let slots: Vec<Mutex<Option<_>>> = work.iter().map(|_| Mutex::new(None)).collect();
        let progress = req.plan_progress.clone();
        if let Some(p) = &progress {
            p.total.store(work.len(), Ordering::Relaxed);
        }
        pool.in_place_scope(|scope| {
            for (item, slot) in work.iter().zip(&slots) {
                #[cfg(feature = "test-hooks")]
                cancel_on_load_for_test(&item.1.path, cancel);
                if cancel.is_cancelled() {
                    continue;
                }
                // DECISIONS AV4: a cache hit loads nothing, so it is admitted at 0 without reading its header.
                // shortcut: a file changed between this check and its Load loads unadmitted; the next plan admits it.
                let path = &item.1.path;
                let hints = hints_of(&ctx, item.1, &split_for(*item));
                let cached =
                    SourceStamp::of(path).is_some_and(|s| cache.file(path, s, hints).is_some());
                let est = if cached { 0 } else { estimated_bytes(path) };
                budget.acquire(est);
                let admitted = Admitted(&budget, est);
                let resolve_one = &resolve_one;
                let progress = &progress;
                scope.spawn(move |_| {
                    if !cancel.is_cancelled() {
                        // Spec §5.9, §8 (final review I1): a panic fails this file alone, at the stage it had
                        // entered, as in `run_one`. The admitted bytes are released while it unwinds.
                        let stage = Cell::new(Stage::Load);
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            resolve_one(item, admitted, &stage)
                        }));
                        *lock(slot) = Some(match r {
                            Ok(r) => r.map_err(PlanFailure::Load),
                            Err(panic) => Err(PlanFailure::Panic(
                                stage.get(),
                                format!("internal error: {}", panic_message(&*panic)),
                            )),
                        });
                        if let Some(p) = progress {
                            p.done.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        // Pass-1 jobs, with the work index, analysis and source info each one needs to be re-resolved (6c).
        let mut pass1_jobs: Vec<ResolvedJob> = vec![];
        let mut pass1 = vec![];
        let mut failed = vec![];
        failed.extend(self_overwrite);
        let mut cancelled = vec![];
        for (wi, (item, slot)) in work.iter().zip(slots).enumerate() {
            let Some(r) = slot.into_inner().unwrap_or_else(|e| e.into_inner()) else {
                cancelled.push(item.1.path.clone());
                continue;
            };
            let path = item.1.path.clone();
            let output = item.1.output.clone();
            match r {
                Ok((job, a, info)) => {
                    pass1_jobs.push(job);
                    pass1.push((wi, a, info));
                }
                Err(PlanFailure::Load(e)) => {
                    rep.emit_job(
                        Some(&key(&output)),
                        None,
                        Some(&path),
                        EventKind::Warning(Warning::new(load_code(&e), e.to_string())),
                    );
                    failed.push(FailedFile {
                        path,
                        output,
                        stage: Stage::Load,
                        error: e.to_string(),
                    });
                }
                Err(PlanFailure::Panic(stage, error)) => failed.push(FailedFile {
                    path,
                    output,
                    stage,
                    error,
                }),
                Err(PlanFailure::Pack(_)) => unreachable!("pass 1 never assembles"),
            }
        }
        // 6a. Explicit shader choices: the GUI map, then `--set set:<id>.shader` (the command line wins).
        for id in &stale_shaders {
            rep.emit(
                None,
                None,
                EventKind::Warning(
                    Warning::new(
                        Code::SetEditStale,
                        format!("shader choice skipped: no set '{id}'"),
                    )
                    .with_data(serde_json::json!({ "set": id })),
                ),
            );
        }
        // 6b. One pick per set, from the pass-1 sub-types; later resolves see it through `ctx.shaders`.
        let index = crate::shader::JobIndex::new(&pass1_jobs);
        let txst: BTreeSet<String> = req
            .cs_txst_sets
            .iter()
            .map(|t| crate::sets::fold_id(t))
            .collect();
        let mut picks: BTreeMap<String, ShaderPick> = sets
            .iter()
            .map(|s| {
                let pick = crate::shader::pick_for_set(
                    &ctx,
                    s,
                    &files,
                    &index,
                    explicit.get(&s.key).copied(),
                    &req.mesh_shaders,
                    &req.gras_all_diffuses,
                    txst.contains(&crate::sets::fold_id(&crate::sets::set_id(
                        &req.output_root,
                        &s.key,
                    ))),
                );
                (s.key.clone(), pick)
            })
            .collect();
        // The pass-1 context (no picks) is kept, so the derive step can rebuild it after adding its picks.
        let base_ctx = ctx;
        let ctx = ResolveCtx {
            shaders: &picks,
            ..base_ctx
        };
        // Note CO103: images whose B is empty, for a generated AO beside a pre-packed `_rmaos` (pack planning).
        let flat_b: BTreeSet<PathBuf> = pass1
            .iter()
            .filter(|(_, a, _)| !pack::b_has_content(&a.channels[2]))
            .map(|(wi, _, _)| work[*wi].1.path.clone())
            .collect();
        // Read by `plan_cubes` (DECISIONS DS8): each input's loaded size and cube-DDS flag.
        let cube_facts: crate::cube::plan::InputFacts = pass1
            .iter()
            .map(|(wi, a, _)| (work[*wi].1.path.clone(), (a.width, a.height, a.dds_cube)))
            .collect();
        // Read by `plan_fills` (Note DY, Ruling 6): each input's alpha verdict on the original source, and its size.
        let alpha_facts: BTreeMap<PathBuf, crate::helper::plan::AlphaFacts> = pass1
            .iter()
            .map(|(wi, a, _)| {
                let empty = matches!(a.alpha, AlphaContent::None | AlphaContent::Constant255)
                    || a.channels[3].min == a.channels[3].max;
                let facts = crate::helper::plan::AlphaFacts {
                    empty,
                    size: (a.width, a.height),
                };
                (work[*wi].1.path.clone(), facts)
            })
            .collect();
        // The load facts a filled target's re-analysis cannot see (it loads pixels only): ICC flag, DDS format.
        let load_meta: BTreeMap<PathBuf, (bool, Option<u32>)> = pass1
            .iter()
            .map(|(wi, a, _)| (work[*wi].1.path.clone(), (a.icc_profile, a.dds_dxgi)))
            .collect();
        // 6c. Re-resolve the members of a set whose shader changes member defaults.
        let mut jobs = vec![];
        for (mut old, (wi, a, info)) in pass1_jobs.into_iter().zip(pass1) {
            // PBR grass design §4.4: the albedo's corner texel against CS's grass shadow-pass detector, judged on the
            // job's final sub-type (a re-resolve can turn a GRAS complex_grass guess into an albedo).
            let grass_note = |j: &ResolvedJob| {
                picks.get(&j.set_key).and_then(|p| {
                    crate::shader::pbr_grass_albedo_note(p, j.profile.subtype, a.corner_err)
                })
            };
            // An explicit set shader also re-resolves a GRAS diffuse: it suppresses the auto-detect (`suggest_grass`).
            let is_gras = gras.contains(&crate::shader::gras_key(
                &work[wi].1.game_path,
                &work[wi].1.output,
            ));
            let (set, f) = work[wi];
            // Note DV1: a chosen Advanced Skin makes the set skin (`chosen_recipe`), so its members are typed again
            // (not while its diffuse is pinned, Note DW1).
            if !picks.get(&old.set_key).is_some_and(|p| {
                p.shader.has_member_defaults()
                    || (p.provenance == crate::shader::ShaderOrigin::Set && is_gras)
            }) && !pack::plan::chosen_skin(&ctx, set)
            {
                old.warnings.extend(grass_note(&old));
                jobs.push(old);
                continue;
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let split = split_for((set, f));
                let facts = FileFacts {
                    input: f,
                    suffix: split.suffix.as_deref(),
                    alias: split.alias.as_deref(),
                    set,
                    analysis: &a,
                    info: &info,
                };
                resolve_file_with(
                    &ctx,
                    &facts,
                    gras.contains(&crate::shader::gras_key(&f.game_path, &f.output)),
                )
            }));
            match r {
                Ok(mut job) => {
                    job.source = old.source;
                    job.warnings.extend(grass_note(&job));
                    job.provenance.insert("target".into(), target_src.clone());
                    jobs.push(job);
                }
                Err(p) => failed.push(FailedFile {
                    path: f.path.clone(),
                    output: f.output.clone(),
                    stage: Stage::Resolve,
                    error: format!("internal error: {}", panic_message(&*p)),
                }),
            }
        }
        // Spec §6.4 (M2.5): channel packing. Pass 1 decides the recipes; pass 2 assembles, analyses and resolves
        // each under the same memory admission and panic isolation as the files above.
        let failed_paths: Vec<PathBuf> = failed.iter().map(|f| f.path.clone()).collect();
        // Note CN26 (O6): the raw heights whose landscape `_p` a Vanilla derive keeps.
        let derive_heights: BTreeSet<PathBuf> = req
            .derive
            .iter()
            .filter(|d| d.mode == crate::derive::DeriveMode::Vanilla)
            .flat_map(|d| &d.sets)
            .filter(|s| s.shader == crate::shader::Shader::PbrLandscape)
            .filter_map(|s| match &s.height {
                Some(pack::ChannelSource::Source {
                    path,
                    role: crate::config::RawMap::Height,
                    ..
                }) => Some(path.clone()),
                _ => None,
            })
            .collect();
        // DECISIONS DS8: six face files become one cube job, and one-file cubes get their layout.
        crate::cube::plan::plan_cubes(&cube_facts, &mut jobs, &mut skipped);
        // Note DY: helper alpha fills, decided before packing so a helper an alpha fill uses is not reported unused.
        let fills = crate::helper::plan::plan_fills(&ctx, &sets, &files, &mut jobs, &alpha_facts);
        let mut packed = pack::plan::plan_packing(
            &ctx,
            &sets,
            &files,
            &mut jobs,
            &failed_paths,
            &cancelled,
            &mut skipped,
            &packed_outputs,
            &derive_heights,
            &flat_b,
            &fills,
        );
        // As step 3b drops its losers, so later steps (the RNM bent AO) never read a replaced generated AO.
        for s in &mut sets {
            s.members
                .retain(|&i| !packed.gen_replaced.contains(&files[i].path));
        }
        // Pass 1 may add a recipe job it fails at once (sources of different sizes); like every job it records the
        // target's origin (`BatchRequest::target`, ruling W1).
        for j in &mut jobs {
            j.provenance
                .entry("target".into())
                .or_insert_with(|| target_src.clone());
        }
        // Pass 2 and the derive step share `admit_all`; the pass-1 block above keeps its own loop (it returns an
        // analysis and source info with each job, and counts progress).
        let pack_results = admit_all(
            &pool,
            &budget,
            cancel,
            &packed.candidates,
            |c| estimated_bytes(&c.job.sources[0].path),
            |c| &c.virtual_input.output,
            |c, admitted, stage| {
                // The key includes every source's stamp, so an edited source misses the cache. A recipe
                // without a key (a source with no stamp) is assembled every time and never stored.
                let key = pack_cache_key(&c.job);
                let hints = crate::checks::hints_for(Some(c.job.kind.suffix()), cache.checks);
                let entry = match key.as_deref().and_then(|k| cache.pack(k, hints)) {
                    Some(e) => {
                        drop(admitted);
                        e
                    }
                    None => {
                        // `check_sizes` (in `assemble`) enforces MAX_DIM per source before the canvas
                        // is allocated; its `TooLarge` maps to DIM-TOO-LARGE like `load_image`'s.
                        let asm = pack::assemble::assemble(&c.job, c.job.gray)
                            .map_err(PlanFailure::of_pack)?;
                        cache.count_loads(c.job.sources.len() as u64);
                        stage.set(Stage::Analyse);
                        let e = PackEntry {
                            analysis: analyse_with(&asm.image, &asm.info, hints),
                            info: asm.info,
                            non_gray: asm.non_gray,
                            thumb: cache.make_thumb(&asm.image, &asm.info),
                            hints,
                        };
                        drop(asm.image);
                        drop(admitted);
                        if let Some(key) = key {
                            cache.put_pack(key, e.clone());
                        }
                        e
                    }
                };
                stage.set(Stage::Resolve);
                let set = sets
                    .iter()
                    .find(|s| s.key == c.set_key)
                    .expect("a candidate's set is planned");
                Ok(pack::plan::finish_recipe(
                    &ctx,
                    c,
                    set,
                    &entry.analysis,
                    &entry.info,
                    &entry.non_gray,
                ))
            },
        );
        // A pass-2 or derive failure as (stage, error).
        let failure = |output: &Path, f: PlanFailure| match f {
            PlanFailure::Load(e) => {
                // The same §10 warning as a pass-1 load failure, so a JSON consumer sees the code; it names the
                // source that failed, the `file_failed` the caller records names the recipe's input.
                let (LoadError::TooLarge { path: bad, .. } | LoadError::Decode { path: bad, .. }) =
                    &e;
                rep.emit_job(
                    Some(&key(output)),
                    None,
                    Some(bad),
                    EventKind::Warning(Warning::new(load_code(&e), e.to_string())),
                );
                (Stage::Load, e.to_string())
            }
            PlanFailure::Pack(e) => (Stage::Load, e),
            PlanFailure::Panic(stage, e) => (stage, e),
        };
        for (c, r) in packed.candidates.iter().zip(pack_results) {
            let path = c.job.sources[0].path.clone();
            let output = c.virtual_input.output.clone();
            let Some(r) = r else {
                // Cancelled before its sources were loaded: the recipe has no job (Note M11, ruling W7).
                pack::plan::fail_report(&mut packed.reports, c, "cancelled".into());
                cancelled.push(path);
                continue;
            };
            match r {
                Ok(mut job) => {
                    // As `resolve_one` does for every other job; `finish_recipe` cannot (it has no `target_src`).
                    job.provenance.insert("target".into(), target_src.clone());
                    jobs.push(job);
                }
                Err(f) => {
                    let (stage, error) = failure(&output, f);
                    pack::plan::fail_report(&mut packed.reports, c, error.clone());
                    failed.push(FailedFile {
                        path,
                        output,
                        stage,
                        error,
                    });
                }
            }
        }
        // Note DY: each filled target is loaded, filled and analysed again, then re-resolved as 6c does, so alpha
        // policy, format, coverage and cutout all see the filled image. It runs before the derive step, so a target
        // a derive replaces loses its fill with it (Ruling 15).
        // shortcut: the filled analysis is not cached (as derive P25); key it on both stamps if re-plans get slow.
        let fill_items: Vec<_> = fills
            .candidates
            .iter()
            .filter_map(|c| {
                let ji = jobs.iter().position(|j| {
                    j.input == c.target
                        && j.pack.is_none()
                        && j.derive.is_none()
                        && j.cube.is_none()
                })?;
                let set = sets.iter().find(|s| s.key == c.set_key)?;
                let f = files.iter().find(|f| f.path == c.target)?;
                Some((c, ji, set, f))
            })
            .collect();
        // Review 4 I1: every consumed helper with the output it fills (a size-mismatch error job carries its `fill`),
        // so one whose target job is later gone (failed, cancelled, derived away) still ends with that output.
        let fill_owner: Vec<(PathBuf, PathBuf, String)> = fill_items
            .iter()
            .map(|&(c, ji, ..)| (&c.fill, &jobs[ji]))
            .chain(
                jobs.iter()
                    .filter_map(|j| j.fill.as_ref().map(|fl| (fl, j))),
            )
            .map(|(fl, j)| (fl.source.path.clone(), j.output.clone(), j.set_key.clone()))
            .collect();
        // Test hook: `SDC_TEST_CANCEL_ON=<target output name>` cancels the batch as the fill step starts (pass 1 is
        // named by inputs, so only this step matches an output name).
        #[cfg(feature = "test-hooks")]
        for &(_, ji, ..) in &fill_items {
            cancel_on_load_for_test(&jobs[ji].output, cancel);
        }
        let fill_results = admit_all(
            &pool,
            &budget,
            cancel,
            &fill_items,
            |(c, ..)| estimated_bytes(&c.target) + estimated_bytes(&c.fill.source.path),
            |(_, ji, ..)| jobs[*ji].output.as_path(),
            |&(c, ji, set, f), admitted, stage| {
                let old = &jobs[ji];
                let split = split_for((set, f));
                let gras_diffuse = gras.contains(&crate::shader::gras_key(&f.game_path, &f.output));
                // Review 4 I2: the resolved profile may change `alpha.exr_straight` (a rule on the filled alpha), so the
                // image is loaded again under the final setting: the analysed image is the one the run writes.
                // shortcut: a second change is not chased (the filled alpha, which rules match, is the same under both
                // settings); loop until stable if a rule ever reads RGB.
                let mut straight = old.profile.alpha.exr_straight;
                let mut tries = 0;
                let (mut job, a) = loop {
                    stage.set(Stage::Load);
                    let (mut img, mut info) = crate::load::load_image_as(&c.target, straight)
                        .map_err(PlanFailure::Load)?;
                    crate::helper::apply_fill(&mut img, &mut info, &c.fill)
                        .map_err(PlanFailure::Pack)?;
                    cache.count_loads(2);
                    stage.set(Stage::Analyse);
                    let hints = crate::checks::hints_for(split.suffix.as_deref(), cache.checks);
                    let mut a = analyse_with(&img, &info, hints);
                    if let Some(&(icc, dxgi)) = load_meta.get(&c.target) {
                        a.icc_profile = icc;
                        a.dds_dxgi = dxgi;
                    }
                    drop(img);
                    stage.set(Stage::Resolve);
                    let facts = FileFacts {
                        input: f,
                        suffix: split.suffix.as_deref(),
                        alias: split.alias.as_deref(),
                        set,
                        analysis: &a,
                        info: &info,
                    };
                    let job = resolve_file_with(&ctx, &facts, gras_diffuse);
                    if job.profile.alpha.exr_straight == straight || tries == 1 {
                        break (job, a);
                    }
                    straight = job.profile.alpha.exr_straight;
                    tries += 1;
                };
                drop(admitted);
                job.source = old.source;
                job.warnings.extend(picks.get(&job.set_key).and_then(|p| {
                    crate::shader::pbr_grass_albedo_note(p, job.profile.subtype, a.corner_err)
                }));
                job.provenance.insert("target".into(), target_src.clone());
                let read = pack::ChannelSource::Source {
                    path: c.fill.source.path.clone(),
                    role: c.fill.source.role,
                    invert: c.fill.invert,
                    channel: c.fill.read.map(|r| r.channel()),
                };
                if let Some(p) = read.provenance(GraySource::R) {
                    job.provenance.insert("fill.a".into(), p);
                }
                job.fill = Some(c.fill.clone());
                Ok(job)
            },
        );
        // A target that failed or was cancelled here has no job, as a pass-2 recipe.
        let mut gone = BTreeSet::new();
        for (&(c, ji, ..), r) in fill_items.iter().zip(fill_results) {
            match r {
                Some(Ok(job)) => jobs[ji] = job,
                Some(Err(e)) => {
                    let output = jobs[ji].output.clone();
                    let (stage, error) = failure(&output, e);
                    failed.push(FailedFile {
                        path: c.target.clone(),
                        output,
                        stage,
                        error,
                    });
                    gone.insert(ji);
                }
                None => {
                    cancelled.push(c.target.clone());
                    gone.insert(ji);
                }
            }
        }
        let mut i = 0;
        jobs.retain(|_| {
            i += 1;
            !gone.contains(&(i - 1))
        });
        // Track A (DECISIONS Note CN): the derive step replaces each PBR set's jobs with derived jobs, after packing
        // (a packed member's job goes too) and before the shader assignment, so a derived set carries its vanilla
        // shader (TA29). Without `derive` nothing below runs (C6).
        let mut derive_own = BTreeSet::new();
        let derived = req.derive.as_ref().map(|d| {
            crate::derive::plan::plan_derived(
                &ctx,
                d,
                &req.mesh_shaders,
                &files,
                &mut jobs,
                &mut skipped,
                &failed,
            )
        });
        if let Some(dp) = derived {
            // The derived jobs resolve under their derived pick.
            picks.extend(dp.picks);
            let ctx = ResolveCtx {
                shaders: &picks,
                ..base_ctx
            };
            // Error jobs (sources of different sizes, TA12 conflicts) record the target, as pass 1's do.
            for j in &mut jobs {
                j.provenance
                    .entry("target".into())
                    .or_insert_with(|| target_src.clone());
            }
            // shortcut: P25: a derived image's analysis is not cached (never under its albedo's path, which would
            // replace the albedo's own), so each plan assembles it again; key it on `identity_sha256` if
            // re-planning an Also export gets slow.
            let results = admit_all(
                &pool,
                &budget,
                cancel,
                &dp.candidates,
                |c| c.job.sources.iter().map(|s| estimated_bytes(&s.path)).sum(),
                |c| &c.input.output,
                |c, admitted, stage| {
                    let (asm, mean_rough) = crate::derive::assemble::assemble_derived_rough(&c.job)
                        .map_err(PlanFailure::of_pack)?;
                    cache.count_loads(c.job.sources.len() as u64);
                    stage.set(Stage::Analyse);
                    let analysis = analyse_with(
                        &asm.image,
                        &asm.info,
                        crate::checks::hints_for(c.job.kind.suffix(), cache.checks),
                    );
                    drop(asm.image);
                    drop(admitted);
                    stage.set(Stage::Resolve);
                    #[cfg(feature = "test-hooks")]
                    panic_while_planning_for_test(&c.input.output, Stage::Resolve);
                    let set = sets
                        .iter()
                        .find(|s| s.key == c.set_key)
                        .expect("a candidate's set is planned");
                    Ok(crate::derive::plan::finish_derived(
                        &ctx, c, set, &analysis, &asm.info, mean_rough,
                    ))
                },
            );
            let failed_before = failed.len();
            for (c, r) in dp.candidates.iter().zip(results) {
                let path = c.input.path.clone();
                match r {
                    None => cancelled.push(path),
                    Some(Ok(mut job)) => {
                        job.provenance.insert("target".into(), target_src.clone());
                        jobs.push(job);
                    }
                    Some(Err(f)) => {
                        let (stage, error) = failure(&c.input.output, f);
                        failed.push(FailedFile {
                            path,
                            output: c.input.output.clone(),
                            stage,
                            error,
                        });
                    }
                }
            }
            if let Some(d) = &req.derive {
                // A derived candidate that failed here ends its own-key source with its `file_failed`.
                let written: BTreeSet<String> = jobs
                    .iter()
                    .map(|j| key(&j.output))
                    .chain(failed[failed_before..].iter().map(|f| key(&f.output)))
                    .collect();
                let members: BTreeSet<&Path> = d
                    .sets
                    .iter()
                    .flat_map(|s| &s.members)
                    .chain(&d.drop)
                    .map(PathBuf::as_path)
                    .collect();
                derive_own = files
                    .iter()
                    .filter(|f| {
                        members.contains(f.path.as_path()) && written.contains(&key(&f.output))
                    })
                    .map(|f| f.path.clone())
                    .collect();
                crate::derive::plan::report_unread(
                    d,
                    &dp.member_sets,
                    &files,
                    &jobs,
                    &mut skipped,
                    &failed[..failed_before],
                    &cancelled,
                    &derive_own,
                );
            }
        }
        let ctx = ResolveCtx {
            shaders: &picks,
            ..base_ctx
        };
        // Spec §6.2–6.3 (M4): the set-level variance bake, after packing so a packed roughness member counts, and
        // before `set_resolved` and `file_planned` so both carry the decision.
        let shared: BTreeSet<String> = req
            .shared_roughness
            .iter()
            .map(|id| crate::sets::set_key(&req.output_root, id))
            .collect();
        let borrowed: BTreeMap<String, String> = req
            .borrowed_normal
            .iter()
            .map(|(b, o)| {
                (
                    crate::sets::set_key(&req.output_root, b),
                    crate::sets::set_key(&req.output_root, o),
                )
            })
            .collect();
        for j in &mut jobs {
            j.shader = picks.get(&j.set_key).cloned();
        }
        // M6 Track F2: RNM layers attach to their base normals once the shaders are known (eye sets are skipped),
        // before the bakes, so a baking roughness member copies its `_n`'s blend (Note W18).
        crate::rnm::plan::plan_rnm(&ctx, &sets, &files, registry.alias_table(), &mut jobs);
        let helpers = crate::helper::report::reports(&ctx, &sets, &files, &jobs, &fills);
        let bake_cx = crate::bake::plan::BakeCtx {
            target,
            sets: &sets,
            files: &files,
            aliases: registry.alias_table(),
            shared: &shared,
            borrowed: &borrowed,
        };
        let mut baking = crate::bake::plan::plan_bakes(&bake_cx, &mut jobs);
        // Note CL40: the opt-in complex `_m` gloss bake, any target.
        baking.extend(crate::bake::plan::plan_gloss_bakes(&bake_cx, &mut jobs));
        let peak_admitted_bytes = budget.peak.load(Ordering::Relaxed);
        // DECISIONS N2: a [files] key that names nothing planned is reported once (it stays in the file).
        let mut rels: Vec<String> = vec![];
        if let Some(root) = &file_scopes.root {
            let rel_of_path = |p: &Path| -> Option<String> {
                let f = InputFile {
                    path: p.to_path_buf(),
                    rel: PathBuf::from(p.file_name()?),
                    game_path: String::new(),
                    output: PathBuf::new(),
                    kind: InputKind::Image,
                    real_rel: None,
                };
                project_rel(root, &f)
            };
            for j in &jobs {
                // A packed output, or a derived `_m` (TA15, Note CN19): the folder of its first source plus the
                // virtual stem (e.g. `rock01_rmaos`, from the first source's name, whatever output path it has),
                // compared with the extension-less key. A packed output's raw sources count too.
                let stem = match (&j.pack, &j.derive) {
                    (Some(pk), _) => Some(crate::pack::plan::virtual_stem(&j.input, pk.kind)),
                    (None, Some(dj)) => dj
                        .kind
                        .suffix()
                        .filter(|s| *s == "m")
                        .map(|s| crate::pack::plan::virtual_stem_as(&j.input, s)),
                    (None, None) => None,
                };
                match stem {
                    Some(stem) => {
                        if let Some(r) = rel_of_path(&j.input) {
                            let dir = r
                                .rsplit_once('/')
                                .map_or(String::new(), |(d, _)| format!("{d}/"));
                            rels.push(format!("{dir}{stem}"));
                        }
                    }
                    None => rels.extend(rel_of_path(&j.input)),
                }
                if let Some(pk) = &j.pack {
                    rels.extend(pk.sources.iter().filter_map(|s| rel_of_path(&s.path)));
                }
                rels.extend(j.fill.iter().filter_map(|f| rel_of_path(&f.source.path)));
            }
            rels.extend(skipped.iter().filter_map(|s| rel_of_path(&s.path)));
            // Note DY: a helper consumed by a fill or a recipe, or one that fills nothing, is a planned file.
            rels.extend(helpers.iter().filter_map(|h| rel_of_path(&h.path)));
            // The derive step removed the jobs of PBR set members (Note CN); their keys still name planned files.
            if let Some(d) = &req.derive {
                let members = d.sets.iter().flat_map(|s| &s.members).chain(&d.drop);
                rels.extend(members.filter_map(|p| rel_of_path(p)));
            }
            rels.extend(failed.iter().filter_map(|f| rel_of_path(&f.path)));
            rels.extend(cancelled.iter().filter_map(|p| rel_of_path(p)));
        }
        // Every form a key can match, so each key is one lookup (DECISIONS AV3).
        let matched: std::collections::HashSet<String> = rels
            .iter()
            .flat_map(|r| {
                let KeyForms { full, stem } = KeyForms::of(r);
                std::iter::once(full).chain(stem)
            })
            .collect();
        let mut keys: BTreeSet<&String> = file_scopes.allow.keys().collect();
        keys.extend(file_scopes.overrides.iter().filter_map(|o| match &o.scope {
            Scope::Path(k) => Some(k),
            _ => None,
        }));
        for key in keys {
            if !matched.contains(&key.to_lowercase()) {
                rep.emit(
                    None,
                    None,
                    EventKind::Warning(
                        Warning::new(
                            Code::CfgFileUnmatched,
                            format!("[files] entry '{key}' matches no file"),
                        )
                        .with_data(serde_json::json!({ "key": key })),
                    ),
                );
            }
        }
        // Audits 08d-1 / 08e-3 (Note BV2): the last word on output paths, whatever made two jobs meet (default
        // packed names, moved raw maps, re-encoded DDS). Both would write the path (the last rename wins) and append
        // two manifest records; the first in plan order keeps it and every later one is IN-DUPLICATE.
        let mut writers: BTreeMap<String, PathBuf> = BTreeMap::new();
        jobs.retain(|j: &ResolvedJob| {
            if !matches!(j.status, JobStatus::Ready) {
                return true;
            }
            let Some(first) = writers.get(&output_key(&req.output_root, &j.output)) else {
                writers.insert(output_key(&req.output_root, &j.output), j.input.clone());
                return true;
            };
            skipped.push(SkippedFile {
                path: j.input.clone(),
                output: j.output.clone(),
                set: Some(j.set_key.clone()),
                code: Some(Code::InDuplicate),
                reason: format!(
                    "two outputs map to one path ({} is also written from {}); change an output path or move a file",
                    slash(&j.output),
                    slash(first)
                ),
            });
            false
        });
        let index = crate::shader::JobIndex::new(&jobs);
        for set in &sets {
            let Some(&first) = set
                .members
                .iter()
                .find(|&&i| files[i].kind == InputKind::Image)
                // Note F?2: an all-DDS set resolves too; `first` only feeds `is_pbr_set`'s game path.
                .or_else(|| set.members.first())
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
                advanced_skin: false,
            });
            let members = set.members.iter().map(|&m| slash(&files[m].path)).collect();
            let pick = picks.get(&set.key).cloned();
            let mut warnings = vec![];
            if let Some(p) = &pick {
                warnings = crate::shader::pick_notes(p, target);
                let maps = map_members(
                    &ctx,
                    set,
                    &files,
                    &index,
                    &skipped,
                    registry.alias_table(),
                    target,
                );
                warnings.extend(crate::shader::check_maps(p, target, &maps));
            }
            rep.emit(
                Some(&set.key),
                None,
                EventKind::SetResolved {
                    members,
                    pbr,
                    bake: baking.contains(&set.key),
                    recipes: packed.reports.remove(&set.key).unwrap_or_default(),
                    shader: pick,
                    warnings,
                },
            );
        }
        for job in &jobs {
            rep.emit_job(
                Some(&key(&job.output)),
                Some(&job.set_key),
                Some(&job.input),
                EventKind::FilePlanned {
                    job: Box::new(job.clone()),
                },
            );
            for w in &job.warnings {
                rep.emit_job(
                    Some(&key(&job.output)),
                    Some(&job.set_key),
                    Some(&job.input),
                    EventKind::Warning(w.clone()),
                );
            }
        }
        // Review 4 I1: a consumed helper that no surviving job, skip, failure or cancellation ends (a helper-free plan
        // skips the walk).
        let fill_ends: Vec<(PathBuf, PathBuf, String)> = if fill_owner.is_empty() {
            vec![]
        } else {
            let ended: BTreeSet<&Path> = jobs
                .iter()
                .flat_map(|j| {
                    let pack = j
                        .pack
                        .iter()
                        .flat_map(|p| p.sources.iter().map(|s| &s.path));
                    let derive = j
                        .derive
                        .iter()
                        .flat_map(|d| d.sources.iter().map(|s| &s.path));
                    let fill = j.fill.iter().map(|f| &f.source.path);
                    std::iter::once(&j.input)
                        .chain(pack)
                        .chain(derive)
                        .chain(fill)
                })
                .chain(skipped.iter().map(|s| &s.path))
                .chain(failed.iter().map(|f| &f.path))
                .chain(&cancelled)
                .chain(&derive_own)
                .map(PathBuf::as_path)
                .collect();
            let mut seen = BTreeSet::new();
            fill_owner
                .into_iter()
                .filter(|(h, ..)| !ended.contains(h.as_path()) && seen.insert(h.clone()))
                .collect()
        };
        Ok(Plan {
            request: req,
            registry,
            preset_id,
            target,
            jobs,
            skipped,
            failed,
            peak_admitted_bytes,
            cancelled,
            shaders: picks,
            nvtt: nvtt.into_probed(),
            derive_own,
            fill_ends,
            helpers,
        })
    }

    /// Report the plan's skipped and failed files, then (unless `dry_run`) convert every ready file and emit
    /// `batch_done`. Cancelled files are listed in `batch_done` and leave no output: the files cancelled while
    /// planning, and once `cancel` is set every ready file not yet started, including one a dry run would only
    /// have listed and one that would have been up to date or kept by `--on-existing`.
    pub fn run(
        &self,
        plan: Plan,
        rep: &Reporter,
        cancel: &CancelToken,
    ) -> Result<BatchSummary, EngineError> {
        let req = &plan.request;
        let nvtt = plan.nvtt.as_ref().and_then(|p| p.tool());
        let mut summary = BatchSummary {
            failed: plan.failed.len() as u32,
            cancelled: plan.cancelled.clone(),
            ..Default::default()
        };
        // Every skip carries the file's set key (review M9).
        let key = |out: &Path| output_key(&req.output_root, out);
        let skip = |output: &Path,
                    set: Option<&str>,
                    path: &Path,
                    kind: SkipKind,
                    code: Option<Code>,
                    reason: String,
                    s: &mut BatchSummary| {
            rep.emit_job(
                Some(&key(output)),
                set,
                Some(path),
                EventKind::FileSkipped { kind, code, reason },
            );
            s.skipped += 1;
        };
        // Note BC2 (review M3): an in-place re-run finds its own earlier outputs as DDS inputs. One whose manifest
        // record still matches the file on disk is counted into one line instead of a skip per file. The manifest is
        // read leniently here; `run` reads it again below and fails there if it is unreadable.
        let recorded = if plan
            .skipped
            .iter()
            .any(|s| s.code == Some(Code::InDdsSkipped))
        {
            Manifest::at(&req.output_root).read().unwrap_or_default()
        } else {
            Default::default()
        };
        let mut earlier = 0usize;
        for s in &plan.skipped {
            if s.code == Some(Code::InDdsSkipped)
                && recorded.get(&key(&s.path)).is_some_and(|e| {
                    e.output_stamp.is_some() && e.output_stamp == SourceStamp::of(&s.path)
                })
            {
                earlier += 1;
                continue;
            }
            skip(
                &s.output,
                s.set.as_deref(),
                &s.path,
                s.kind(),
                s.code,
                s.reason.clone(),
                &mut summary,
            );
            // IN-DUPLICATE is a conflict the user must resolve; IN-DDS-SKIPPED (Note I) and a pre-packed file
            // replaced under `packing.prefer = raw` (Note M9) are deliberate, as is a PBR set left in DDS (Note CN16).
            if matches!(
                s.code,
                Some(
                    Code::InDdsSkipped
                        | Code::PackConflict
                        | Code::DeriveDdsSource
                        | Code::DeriveNotConverted
                        | Code::GenReplaced
                )
            ) {
                summary.skipped_intentional += 1;
            }
        }
        if earlier > 0 {
            rep.emit(
                None,
                None,
                EventKind::Warning(
                    Warning::new(
                        Code::InDdsSkipped,
                        format!(
                            "{earlier} earlier outputs in the input folder were not re-read (DDS inputs are not re-encoded by default)"
                        ),
                    )
                    .with_data(serde_json::json!({ "count": earlier })),
                ),
            );
        }
        for f in &plan.failed {
            rep.emit_job(
                Some(&key(&f.output)),
                None,
                Some(&f.path),
                EventKind::FileFailed {
                    stage: f.stage,
                    error: f.error.clone(),
                },
            );
        }
        // Note DY (review 4 I1): a helper whose target has no job ends with the output it filled.
        for (helper, output, set) in &plan.fill_ends {
            rep.emit_job(
                Some(&key(output)),
                Some(set),
                Some(helper),
                EventKind::FileSkipped {
                    kind: SkipKind::PackedSource,
                    code: None,
                    reason: format!("filled into {}", slash(output)),
                },
            );
        }
        let mut ready = vec![];
        // A file a recipe packs (the kept landscape `_p`'s raw height, Note CN26) already ends with that event.
        let mut derive_read: BTreeSet<&Path> = plan
            .derive_own
            .iter()
            .map(PathBuf::as_path)
            .chain(
                plan.jobs
                    .iter()
                    .filter_map(|j| j.pack.as_ref())
                    .flat_map(|pk| pk.sources.iter().map(|s| s.path.as_path())),
            )
            .collect();
        for job in &plan.jobs {
            let (set, path) = (Some(job.set_key.as_str()), Some(job.input.as_path()));
            // A raw map consumed by a recipe has no job of its own: its terminal event names the packed output. So
            // does each file a derived job reads (Note CN23), once, naming the first derived output it feeds.
            let consumed = job
                .pack
                .iter()
                .flat_map(|pk| pk.sources.iter().map(|s| (&s.path, "packed")))
                .chain(
                    job.derive
                        .iter()
                        .flat_map(|d| d.sources.iter().map(|s| (&s.path, "derived")))
                        // Note DY: so does a helper, with the first output it fills, unless a recipe packs it too.
                        .chain(job.fill.iter().map(|f| (&f.source.path, "filled")))
                        .filter(|(p, _)| derive_read.insert(p.as_path())),
                )
                // DECISIONS DS8: each face of a face-set cube (a broken set lists its lead file only), keyed by the
                // cube's output like a packed source.
                .chain(
                    job.cube
                        .iter()
                        .filter(|c| c.layout == crate::config::CubeLayout::Faces)
                        .flat_map(|c| c.sources.iter().map(|s| (&s.path, "cubed"))),
                );
            for (path, how) in consumed {
                rep.emit_job(
                    Some(&key(&job.output)),
                    set,
                    Some(path),
                    EventKind::FileSkipped {
                        kind: SkipKind::PackedSource,
                        code: None,
                        reason: format!("{how} into {}", slash(&job.output)),
                    },
                );
            }
            match &job.status {
                JobStatus::Ready => ready.push(job),
                JobStatus::Unresolved { reasons, .. } => skip(
                    &job.output,
                    set,
                    &job.input,
                    SkipKind::Unresolved,
                    Some(unresolved_code(job)),
                    reasons.join("; "),
                    &mut summary,
                ),
                // Spec §4.4/§6.4 (DECISIONS Note M4): a raw map that is not packed is reported, with its
                // IN-PACKING-INPUT or PACK-UNUSED-INPUT warning, but does not make the batch partial.
                JobStatus::PackingInput { role } => {
                    let w = job
                        .warnings
                        .iter()
                        .find(|w| matches!(w.code, Code::InPackingInput | Code::PackUnusedInput));
                    rep.emit_job(
                        Some(&key(&job.output)),
                        set,
                        path,
                        EventKind::FileSkipped {
                            kind: SkipKind::PackingInput,
                            code: Some(w.map_or(Code::InPackingInput, |w| w.code)),
                            reason: w.map_or_else(
                                || format!("raw {} map, not converted on its own", role.id()),
                                |w| w.message.clone(),
                            ),
                        },
                    )
                }
                // M6 Track F2: a layer is reported once, with its RNM-* warning when it feeds nothing.
                JobStatus::NormalLayer { .. } => {
                    let w = job
                        .warnings
                        .iter()
                        .find(|w| w.code.as_str().starts_with("RNM-"));
                    rep.emit_job(
                        Some(&key(&job.output)),
                        set,
                        path,
                        EventKind::FileSkipped {
                            kind: SkipKind::NormalLayer,
                            code: w.map(|w| w.code),
                            reason: w.map_or_else(
                                || "RNM normal layer, used through its base normal".to_string(),
                                |w| w.message.clone(),
                            ),
                        },
                    )
                }
                JobStatus::Blocked { codes } => skip(
                    &job.output,
                    set,
                    &job.input,
                    SkipKind::Blocked,
                    codes.first().copied(),
                    format!(
                        "blocked by {}; acknowledge with --allow <code> or change the setting",
                        codes
                            .iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    &mut summary,
                ),
                // Spec §10: an error-severity code fails the file.
                JobStatus::Error { codes } => {
                    summary.failed += 1;
                    rep.emit_job(
                        Some(&key(&job.output)),
                        set,
                        path,
                        EventKind::FileFailed {
                            stage: Stage::Resolve,
                            error: format!(
                                "resolve error: {}",
                                codes
                                    .iter()
                                    .map(|c| c.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        },
                    );
                }
            }
        }
        if req.dry_run {
            if cancel.is_cancelled() {
                summary
                    .cancelled
                    .extend(ready.iter().map(|j| j.input.clone()));
            }
            summary.cancelled.sort();
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

        let pool = thread_pool(req.jobs)?;
        // Audit 9 (DECISIONS Note Y3): the if-changed checks hash every source; run them on the pool, then report
        // them below in plan order.
        let current: Vec<bool> = if req.on_existing == OnExisting::IfChanged {
            use rayon::prelude::*;
            pool.install(|| {
                ready
                    .par_iter()
                    .map(|job| {
                        !cancel.is_cancelled()
                            && job.output.exists()
                            && existing
                                .get(&output_key(&req.output_root, &job.output))
                                .is_some_and(|e| {
                                    is_up_to_date(
                                        e,
                                        job,
                                        &encoders,
                                        req.bc1_model,
                                        nvtt.map(|t| t.version.as_str()),
                                    )
                                })
                    })
                    .collect()
            })
        } else {
            vec![false; ready.len()]
        };
        // The bool is measure-only: an up-to-date output re-measured under `LowDetailMode::Hold`, never encoded.
        let mut todo: Vec<(&ResolvedJob, PresetRef, bool)> = vec![];
        for (job, current) in ready.into_iter().zip(current) {
            let mut measure_only = false;
            if cancel.is_cancelled() {
                summary.cancelled.push(job.input.clone());
                continue;
            }
            if job.output.exists() {
                match req.on_existing {
                    OnExisting::Overwrite => {}
                    OnExisting::Skip => {
                        skip(
                            &job.output,
                            Some(&job.set_key),
                            &job.input,
                            SkipKind::OnExistingSkip,
                            None,
                            "output exists (--on-existing skip)".into(),
                            &mut summary,
                        );
                        summary.skipped_intentional += 1;
                        continue;
                    }
                    OnExisting::IfChanged => {
                        match existing.get(&output_key(&req.output_root, &job.output)) {
                            None => {
                                rep.emit_job(
                                Some(&key(&job.output)),
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
                                    &job.output,
                                    Some(&job.set_key),
                                    &job.input,
                                    SkipKind::UnknownExisting,
                                    Some(Code::OutUnknownExisting),
                                    "existing output without a manifest record".into(),
                                    &mut summary,
                                );
                                continue;
                            }
                            // Note CL20 (owner answer 2): "flagged again next time" — Hold re-measures an eligible
                            // up-to-date output; the size half of eligibility is known only after Load.
                            Some(e)
                                if current
                                    && (!req.sidecar || sidecar_present(&job.output, e))
                                    && req.low_detail == LowDetailMode::Hold
                                    && crate::checks::low_detail_profile_eligible(&job.profile) =>
                            {
                                measure_only = true;
                            }
                            Some(e)
                                if current && (!req.sidecar || sidecar_present(&job.output, e)) =>
                            {
                                summary.up_to_date += 1;
                                rep.emit_job(
                                    Some(&key(&job.output)),
                                    Some(&job.set_key),
                                    Some(&job.input),
                                    EventKind::FileSkipped {
                                        kind: SkipKind::UpToDate,
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
                Some(p) => todo.push((job, p, measure_only)),
                None => {
                    summary.failed += 1;
                    rep.emit_job(
                        Some(&key(&job.output)),
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

        let budget = Budget::new(budget_limit(req), pool.current_num_threads());
        let state = Mutex::new(summary);
        // Admission runs on the calling thread (outside the pool) and only spawns admitted files, so no pool thread
        // ever blocks on the budget. A pool thread blocked in `acquire` while its nested band-parallel encode stole
        // another file's task could otherwise hold budget forever (deadlock).
        pool.in_place_scope(|scope| {
            for &(job, ref preset, measure_only) in &todo {
                if cancel.is_cancelled() {
                    lock(&state).cancelled.push(job.input.clone());
                    continue;
                }
                let est = job_bytes(job);
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
                    run_one(
                        job,
                        req,
                        preset,
                        measure_only,
                        cancel,
                        rep,
                        manifest,
                        state,
                        nvtt,
                    );
                });
            }
        });
        let mut summary = state.into_inner().unwrap_or_else(|e| e.into_inner());
        summary.cancelled.sort();
        rep.emit(None, None, done_event(&summary));
        Ok(summary)
    }
}

/// Convert one admitted file under `catch_unwind` and report its outcome. A file that fails or panics after its
/// output was written (manifest append; a sidecar failure is only a warning, Note BV4) has that output, and a sidecar this run wrote, removed, best effort, so no output is
/// left on disk without a record (review I1). A panic is reported at the last stage the file entered (review M10).
#[allow(clippy::too_many_arguments)]
fn run_one(
    job: &ResolvedJob,
    req: &BatchRequest,
    preset: &PresetRef,
    measure_only: bool,
    cancel: &CancelToken,
    rep: &Reporter,
    manifest: &Manifest,
    state: &Mutex<BatchSummary>,
    nvtt: Option<&crate::nvtt::NvttTool>,
) {
    let (set, path) = (Some(job.set_key.as_str()), Some(job.input.as_path()));
    let key = output_key(&req.output_root, &job.output);
    let emit = |kind: EventKind| rep.emit_job(Some(&key), set, path, kind);
    emit(EventKind::FileStarted);
    let tracker = FileTracker::default();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_file(
            job,
            req,
            preset,
            measure_only,
            cancel,
            rep,
            manifest,
            &tracker,
            nvtt,
        )
    }));
    let (stage, error) = match r {
        Ok(Ok(Processed::UpToDate(w))) => {
            // Note FC2: a flagged up-to-date output keeps its full chain; the warning is the record.
            if let Some(w) = w {
                emit(EventKind::Warning(w));
            }
            lock(state).up_to_date += 1;
            emit(EventKind::FileSkipped {
                kind: SkipKind::UpToDate,
                code: None,
                reason: "up to date".into(),
            });
            return;
        }
        Ok(Ok(Processed::Written(out))) => {
            for w in &out.report.warnings {
                emit(EventKind::Warning(w.clone()));
            }
            {
                let mut s = lock(state);
                s.converted += 1;
                if !out.report.summary.passed {
                    s.validation_failed += 1;
                }
            }
            emit(EventKind::FileDone {
                output: slash(&out.output),
                validation: out.report.summary,
            });
            return;
        }
        Ok(Err(FileError::Cancelled)) => {
            lock(state).cancelled.push(job.input.clone());
            return;
        }
        Ok(Err(FileError::Failed(stage, error))) => (stage, error),
        Err(panic) => (
            tracker.stage.get(),
            format!("internal error: {}", panic_message(&*panic)),
        ),
    };
    if tracker.written.get() {
        let _ = std::fs::remove_file(&job.output);
        if tracker.sidecar_written.get() {
            let _ = std::fs::remove_file(job.output.with_extension("dds.json"));
        }
    }
    lock(state).failed += 1;
    emit(EventKind::FileFailed { stage, error });
}

/// Planning step 6a (DECISIONS Note S-shader-1/-2): set key → explicit shader, from `req.set_shaders` then the
/// `set:` overrides in command-line order (later wins). An id names a set by `sets::set_id`; a command-line id
/// without `/` also names every set whose key ends in `/<id>`. GUI ids are canonical and match exactly (final
/// review I2). Also returns each id that names no set, once, for one SET-EDIT-STALE each at step 6a. Computed
/// before the packing prediction (step 6), which reads it (Note AZ).
fn explicit_shaders(
    req: &BatchRequest,
    sets: &[TextureSet],
) -> (BTreeMap<String, Shader>, Vec<String>) {
    let gui = req.set_shaders.iter().map(|(k, s)| (k, *s, false));
    let cli = req.overrides.iter().filter_map(|o| match &o.scope {
        Scope::Set(id) => Some((
            id,
            serde_json::from_value::<Shader>(o.value.clone()).ok()?,
            true,
        )),
        _ => None,
    });
    let mut out = BTreeMap::new();
    let mut stale = vec![];
    for (id, shader, tail_match) in gui.chain(cli) {
        let tail = format!("/{}", crate::sets::fold_id(id));
        let mut hit = false;
        for s in sets {
            if crate::sets::same_id(&crate::sets::set_id(&req.output_root, &s.key), id)
                || (tail_match && !id.contains('/') && s.key.ends_with(&tail))
            {
                out.insert(s.key.clone(), shader);
                hit = true;
            }
        }
        if !hit && !stale.contains(id) {
            stale.push(id.clone());
        }
    }
    (out, stale)
}

/// The members the shader map check sees (spec §5): each non-raw member with its ordinary job (or as a kept DDS
/// input with the sub-type its suffix suggests, not the set's own earlier output), then each packed output of the
/// set. Raw maps are left out by role, because
/// `plan_packing` re-tags an unused one; duplicates and failed files are left out. A raw-named image set to
/// `helper.role = "none"` (Note DY) is an ordinary member.
fn map_members(
    ctx: &ResolveCtx,
    set: &TextureSet,
    files: &[InputFile],
    jobs: &crate::shader::JobIndex,
    skipped: &[SkippedFile],
    aliases: &BTreeMap<String, String>,
    target: Target,
) -> Vec<crate::shader::MemberMap> {
    let name = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let suffix = |f: &InputFile| {
        let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy();
        member_split(&stem, set, aliases).suffix
    };
    let mut out = vec![];
    for &i in &set.members {
        let f = &files[i];
        if pack::plan::raw_role(f).is_some()
            && crate::policy::helper_requested(ctx, f).role
                != Some(crate::helper::HelperRole::NotHelper)
        {
            continue;
        }
        // Trace-debug #25 (Note BQ): an RNM layer or mask is never converted on its own, as the shader pick knows.
        if let Some(j) = jobs.by_input(&f.path).find(|j| j.pack.is_none()) {
            // Note DR: a skin `_f` packing input has no file-name role, so its job status leaves it out.
            if matches!(
                j.status,
                JobStatus::NormalLayer { .. } | JobStatus::PackingInput { .. }
            ) {
                continue;
            }
            out.push(crate::shader::MemberMap {
                name: name(&j.output),
                suffix: suffix(f),
                subtype: Some(j.profile.subtype),
            });
        } else if let Some(s) = skipped
            .iter()
            .find(|s| s.path == f.path && s.code == Some(Code::InDdsSkipped))
        {
            // Audit 08b-2: an in-place run's own earlier output is not a member map; any other kept DDS fills the
            // slot its suffix suggests (`_n.dds` fills `_n`).
            if jobs
                .in_set(&set.key)
                .any(|j| crate::dds::same_file(&f.path, &j.output))
            {
                continue;
            }
            let sfx = suffix(f);
            let subtype = crate::suggest::suggest(&SuggestContext {
                target,
                suffix: sfx.as_deref(),
                game_path: &f.game_path,
                set_suffixes: &set.suffixes,
                alpha: AlphaContent::None,
                is_float: false,
                advanced_skin: false,
            })
            .first()
            .map(|s| s.subtype);
            out.push(crate::shader::MemberMap {
                name: name(&s.output),
                suffix: sfx,
                subtype,
            });
        }
    }
    out.extend(jobs.in_set(&set.key).filter(|j| j.pack.is_some()).map(|j| {
        crate::shader::MemberMap {
            name: name(&j.output),
            suffix: None,
            subtype: Some(j.profile.subtype),
        }
    }));
    out
}

/// The text of a caught panic's payload (`panic!` with a literal or a formatted message), or `panic`.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Test hook (`test-hooks` feature only): when `SDC_TEST_CANCEL_ON` names an input's file name, planning cancels
/// the batch just before that file's Load, as a Ctrl-C would, so the CLI's cancelled-plan path can be tested
/// without a console.
#[cfg(feature = "test-hooks")]
fn cancel_on_load_for_test(path: &Path, cancel: &CancelToken) {
    if std::env::var_os("SDC_TEST_CANCEL_ON")
        .is_some_and(|v| path.file_name() == Some(v.as_os_str()))
    {
        cancel.cancel();
    }
}

/// Test hook (`test-hooks` feature only, final review I1): when `SDC_TEST_PANIC_ON_PLAN` is `<file name>` or
/// `<file name>@<stage>` (`load`, the default, `analyse` or `resolve`), planning panics as that input enters that
/// stage, to prove per-file panic isolation while planning.
#[cfg(feature = "test-hooks")]
fn panic_while_planning_for_test(path: &Path, stage: Stage) {
    let Some(v) = std::env::var_os("SDC_TEST_PANIC_ON_PLAN") else {
        return;
    };
    let v = v.to_string_lossy();
    let (name, at) = v.split_once('@').unwrap_or((&v, "load"));
    let at = match at {
        "analyse" => Stage::Analyse,
        "resolve" => Stage::Resolve,
        _ => Stage::Load,
    };
    if stage == at
        && path
            .file_name()
            .is_some_and(|n| n.to_string_lossy() == name)
    {
        panic!("test hook: SDC_TEST_PANIC_ON_PLAN");
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
    // DECISIONS N2: the project config's `[files]` entries are held to the same checks, as config errors.
    let mut file_overrides = vec![];
    for c in configs {
        file_overrides.extend(c.file_entries().map_err(config_err)?.0);
    }
    for o in file_overrides.iter().filter(|o| o.field == "subtype") {
        if let Scope::Path(key) = &o.scope {
            serde_json::from_value::<Subtype>(o.value.clone()).map_err(|_| {
                EngineError::Config(format!(
                    "[files.\"{key}\"] subtype: unknown sub-type {}",
                    o.value
                ))
            })?;
        }
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
    // Audit #7 (Note BC): a glob that does not compile would match nothing, silently.
    for o in &req.overrides {
        if let Scope::File(g) = &o.scope {
            crate::policy::compile_glob(g)
                .map_err(|e| usage(format!("--set file:{g}.{}: {e}", o.field)))?;
        }
    }
    // Audit EC-6 (Note BJ): the rule and profile checks are `presets validate`'s, for the batch chain with the
    // configs and for every suffix alias preset's chain.
    check_chain(chain, configs, target).map_err(config_err)?;
    for (pid, c) in &alias_chains {
        check_chain(c, configs, target)
            .map_err(|e| EngineError::Config(format!("suffix alias preset '{pid}': {e}")))?;
    }
    for st in Subtype::ALL {
        let doc = chain_doc(chain, configs, st);
        for o in req.overrides.iter().filter(|o| {
            o.field != "subtype"
                && !crate::overrides::is_selector(&o.field)
                && !matches!(o.scope, Scope::Set(_))
        }) {
            let mut d = doc.clone();
            merge_json(&mut d, &to_patch(&o.field, &o.value));
            finish_profile(d, target, st)
                .map_err(|e| usage(format!("--set {}={}: {e}", o.field, o.value)))?;
        }
        for o in file_overrides
            .iter()
            .filter(|o| o.field != "subtype" && !crate::overrides::is_selector(&o.field))
        {
            let Scope::Path(key) = &o.scope else { continue };
            let mut d = doc.clone();
            merge_json(&mut d, &to_patch(&o.field, &o.value));
            finish_profile(d, target, st).map_err(|e| {
                EngineError::Config(format!("[files.\"{key}\"] {}={}: {e}", o.field, o.value))
            })?;
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
        skipped_intentional: s.skipped_intentional,
        failed: s.failed,
        validation_failed: s.validation_failed,
        cancelled: s.cancelled.iter().map(|p| slash(p)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ENGINE_VERSION;

    /// DECISIONS DS7: admission counts a cube DDS as the 6w x h strip it loads as, in both header forms.
    #[test]
    fn cube_dds_input_dims_are_the_loaded_strip() {
        use crate::dds::{dds_bytes_cube, DdsDesc, ResolvedHeader};
        let d = tempfile::tempdir().unwrap();
        for header in [ResolvedHeader::Dx10, ResolvedHeader::Legacy] {
            let desc = DdsDesc {
                width: 8,
                height: 8,
                format: crate::config::Format::Rgba8,
                srgb: false,
                header,
            };
            let levels: Vec<Vec<u8>> = (0..6)
                .flat_map(|_| [8usize, 4, 2, 1].map(|s| vec![0u8; s * s * 4]))
                .collect();
            let p = d.path().join(format!("c{header:?}.dds"));
            std::fs::write(&p, dds_bytes_cube(&desc, &levels).unwrap()).unwrap();
            let (img, ..) = crate::load::load_image_meta(&p).unwrap();
            assert_eq!(input_dims(&p), Some((img.width, img.height)), "{header:?}");
            assert_eq!(input_dims(&p), Some((48, 8)));
        }
    }

    /// Final review I1: a single-photo cube also holds its fake 2w x w float panorama (`import_dir::photo_panorama`).
    #[test]
    fn a_photo_cube_is_admitted_for_its_panorama() {
        let d = tempfile::tempdir().unwrap();
        let mut j = png_job(d.path(), "sky_e.png", (64, 36), Subtype::Cubemap);
        let base = job_bytes(&j);
        let cube = |layout| crate::cube::CubeJob {
            layout,
            sources: vec![crate::cube::CubeSource {
                path: j.input.clone(),
                stamp: None,
                face: None,
            }],
        };
        let face = |layout| u64::from(crate::cube::layout::face_size(&j.profile, layout, (64, 36)));
        let photo_face = face(CubeLayout::Photo);
        j.cube = Some(cube(CubeLayout::Photo));
        assert_eq!(
            job_bytes(&j),
            base + 6 * photo_face * photo_face * 16 * 3 + 2 * 64 * 64 * 16
        );
        let eq_face = face(CubeLayout::Equirect);
        j.cube = Some(cube(CubeLayout::Equirect));
        assert_eq!(job_bytes(&j), base + 6 * eq_face * eq_face * 16 * 3);
    }

    #[test]
    fn exit_code_precedence() {
        let mut s = BatchSummary::default();
        assert_eq!(s.exit_code(), 0);
        s.up_to_date = 3;
        assert_eq!(s.exit_code(), 0);
        (s.skipped, s.skipped_intentional) = (2, 2);
        assert_eq!(s.exit_code(), 0);
        s.skipped = 3;
        assert_eq!(s.exit_code(), 1);
        s.skipped = 2;
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
        let b = Budget::new(10, 8);
        b.acquire(100);
        assert_eq!(*lock(&b.used), (100, 1));
        drop(Admitted(&b, 100));
        assert_eq!(*lock(&b.used), (0, 0));
        b.acquire(4);
        b.acquire(6);
        assert_eq!(*lock(&b.used), (10, 2));
    }

    /// Review Focus 4 (Task 7 review I2): a 16K `_n` (16-bit RGBA, the widest integer source) baking into a 4K
    /// roughness member is admitted for at least its peak. The `_n` decode (decoded image 8 + `to_rgba32f` 16 + the
    /// collected `Vec` 16 per texel) and the member's own conversion (with the 1.33 B/texel pyramid) never overlap.
    #[test]
    fn a_16k_normal_baking_into_a_4k_member_is_admitted_for_its_peak() {
        let (n, m) = (16384u64 * 16384, 4096u64 * 4096);
        let admitted = input_bytes(4096, 4096) + crate::bake::BakeJob::peak_bytes(16384, 16384, 8);
        let decode_peak = n * (8 + 16 + 16);
        let member_peak = input_bytes(4096, 4096) + n * 4 / 3 + 1;
        assert!(admitted >= decode_peak, "{admitted} < {decode_peak}");
        assert!(admitted >= member_peak, "{admitted} < {member_peak}");
        assert!(m * 64 <= input_bytes(4096, 4096));
    }

    /// Fix wave (final review I2): a re-encoded DDS is admitted for its size, read from its header, plus its file
    /// bytes; a non-DDS `.dds` and a hostile size do not panic.
    #[test]
    fn a_dds_input_is_admitted_for_its_header_size_plus_its_file() {
        use crate::dds::*;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.DDS");
        let desc = DdsDesc {
            width: 64,
            height: 32,
            format: crate::config::Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        };
        let b = dds_bytes(&desc, &[vec![0; 64 * 32]]).unwrap();
        std::fs::write(&p, &b).unwrap();
        assert_eq!(estimated_bytes(&p), input_bytes(64, 32) + b.len() as u64);
        let mut huge = b.clone();
        huge[12..20].fill(0xff);
        std::fs::write(&p, &huge).unwrap();
        assert_eq!(
            estimated_bytes(&p),
            input_bytes(crate::load::MAX_DIM, crate::load::MAX_DIM) + b.len() as u64
        );
        std::fs::write(&p, b"not a dds").unwrap();
        assert_eq!(estimated_bytes(&p), 0);
    }

    /// Residual fix (re-review minor 1): a non-DDS input's size is read by the decoder its content picks, so a
    /// PNG named `.bmp` is admitted for its size, not 0.
    #[test]
    fn a_non_dds_input_is_admitted_for_its_size_by_content() {
        let d = tempfile::tempdir().unwrap();
        let png = d.path().join("a.png");
        image::GrayImage::new(64, 32).save(&png).unwrap();
        let lying = d.path().join("a.bmp");
        std::fs::rename(&png, &lying).unwrap();
        assert_eq!(estimated_bytes(&lying), input_bytes(64, 32));
    }

    /// A Cs job of `subtype` reading a `w`×`h` RGB PNG at `dir/name`.
    fn png_job(dir: &Path, name: &str, (w, h): (u32, u32), subtype: Subtype) -> ResolvedJob {
        let input = dir.join(name);
        image::RgbImage::new(w, h).save(&input).unwrap();
        ResolvedJob {
            input,
            output: dir.join("out.dds"),
            game_path: "a".into(),
            set_key: "x/a".into(),
            profile: PresetRegistry::builtin()
                .unwrap()
                .base_profile(Target::Cs, subtype),
            provenance: Default::default(),
            suggestions: vec![],
            warnings: vec![],
            status: JobStatus::Ready,
            suffix_alias: None,
            source: None,
            pack: None,
            bake: None,
            rnm: None,
            derive: None,
            shader: None,
            cube: None,
            fill: None,
        }
    }

    /// Note BB: a 64 `_n` with a 256 detail layer (the 8K-on-2K ratio, 4x) and the base-only bytes of that job.
    fn layered_normal(dir: &Path) -> (ResolvedJob, u64) {
        let mut j = png_job(dir, "a_n.png", (64, 64), Subtype::Normal);
        let base = job_bytes(&j);
        let layer = dir.join("detail.png");
        image::RgbImage::new(256, 256).save(&layer).unwrap();
        j.rnm = Some(crate::rnm::RnmJob {
            layers: vec![crate::rnm::RnmLayer {
                id: LayerId::Detail,
                file: crate::rnm::RnmFile {
                    path: layer,
                    stamp: None,
                },
                mask: None,
            }],
            ao: None,
        });
        (j, base)
    }

    /// Note BB: a layered normal is admitted for its largest layer's decode (8-bit RGB: 3 + 16 + 16 per layer texel)
    /// plus that layer resampled to the base (16 per base texel), on top of the base.
    #[test]
    fn a_layer_bigger_than_its_normal_is_admitted_for_its_decode_and_resample() {
        let d = tempfile::tempdir().unwrap();
        let (mut j, base) = layered_normal(d.path());
        assert_eq!(job_bytes(&j), base + 256 * 256 * 35 + 64 * 64 * 16);
        // Off, the layers are not loaded, so they are not admitted.
        j.profile.rnm.enabled = false;
        assert_eq!(job_bytes(&j), base);
    }

    /// Note BB: under a budget that holds two base-only jobs, two layered ones run one at a time.
    #[test]
    fn two_layered_normals_are_not_admitted_together_under_a_small_budget() {
        let d = tempfile::tempdir().unwrap();
        let (j, base) = layered_normal(d.path());
        let n = job_bytes(&j);
        // One layered job fits, two do not; two base-only jobs would.
        let b = Budget::new(n + base, 8);
        b.acquire(n);
        let admitted = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                b.acquire(n);
                admitted.store(true, Ordering::SeqCst);
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(
                !admitted.load(Ordering::SeqCst),
                "second layered job admitted beside the first"
            );
            b.release(n);
        });
        assert!(admitted.load(Ordering::SeqCst));
        assert_eq!(*lock(&b.used), (n, 1));
    }

    /// Note BB: a layered job bigger than the whole budget is admitted alone, as any oversized file is.
    #[test]
    fn a_layered_normal_over_the_whole_budget_still_runs_alone() {
        let d = tempfile::tempdir().unwrap();
        let (j, base) = layered_normal(d.path());
        let n = job_bytes(&j);
        assert!(n > 2 * base, "{n} <= {}", 2 * base);
        let b = Budget::new(2 * base, 8);
        b.acquire(n);
        assert_eq!(*lock(&b.used), (n, 1));
    }

    /// Note BB: complex grass holds both half chains and the stacked chain at once, one float chain (16 B/texel ×
    /// 1.34) more than the generic model; about 1.33x the generic admission.
    #[test]
    fn complex_grass_is_admitted_for_its_chain_peak() {
        let d = tempfile::tempdir().unwrap();
        let mut j = png_job(d.path(), "g.png", (32, 64), Subtype::ComplexGrass);
        let grass = job_bytes(&j);
        j.profile.subtype = Subtype::Diffuse;
        let generic = job_bytes(&j);
        assert_eq!(grass, generic + 32 * 64 * 16 * 134 / 100);
        let ratio = grass as f64 / input_bytes(32, 64) as f64;
        assert!((1.3..=1.5).contains(&ratio), "{ratio}");
    }

    /// Note DY: a filled job also holds its helper's decoded image.
    #[test]
    fn job_bytes_counts_helper() {
        let d = tempfile::tempdir().unwrap();
        let mut j = png_job(d.path(), "x.png", (32, 32), Subtype::Diffuse);
        let base = job_bytes(&j);
        let helper = d.path().join("x_opacity.png");
        image::GrayImage::new(16, 16).save(&helper).unwrap();
        j.fill = Some(crate::helper::FillJob {
            source: crate::pack::PackSource {
                read: None,
                path: helper.clone(),
                role: crate::config::RawMap::Opacity,
                stamp: None,
            },
            read: None,
            invert: false,
            assigned: false,
        });
        assert!(estimated_bytes(&helper) > 0);
        assert_eq!(job_bytes(&j), base + estimated_bytes(&helper));
    }

    /// Note CN13: a derived job adds 16 B/texel per colour map, the scalars and the output (twice for the grass
    /// stack, which reads the albedo and the normal).
    #[test]
    fn a_derived_job_is_admitted_for_its_colour_scalars_and_output() {
        use crate::derive::{DeriveJob, DeriveKind};
        use crate::pack::ChannelSource::Default;
        let d = tempfile::tempdir().unwrap();
        let mut j = png_job(d.path(), "a_n.png", (32, 32), Subtype::Normal);
        let base = job_bytes(&j);
        let dj = |kind| DeriveJob {
            kind,
            landscape: false,
            albedo: None,
            normal: None,
            rough: Default { value: 0.5 },
            metal: Default { value: 0.0 },
            ao: Default { value: 1.0 },
            height: None,
            curves: None,
            cavity: None,
            gray: crate::config::GraySource::R,
            params: crate::derive::recipe::LegacyParams::default(),
            sources: vec![],
        };
        j.derive = Some(dj(DeriveKind::LegacyN));
        assert_eq!(job_bytes(&j), base + 32 * 32 * 16 * 3);
        j.derive = Some(dj(DeriveKind::GrassStack));
        assert_eq!(job_bytes(&j), base + 32 * 32 * 16 * 5);
    }

    /// S-mips-11: a non-box filter adds its 8 B/texel intermediate, a resize its 32 B/texel; a DDS is sized from
    /// its header for both (ruling F4).
    #[test]
    fn admission_counts_the_filter_and_resize_intermediates() {
        let d = tempfile::tempdir().unwrap();
        let png = d.path().join("a.png");
        image::GrayImage::new(48, 40).save(&png).unwrap();
        let mut job = ResolvedJob {
            input: png,
            output: d.path().join("a.dds"),
            game_path: "a".into(),
            set_key: "x/a".into(),
            profile: PresetRegistry::builtin()
                .unwrap()
                .base_profile(Target::Cs, Subtype::Diffuse),
            provenance: Default::default(),
            suggestions: vec![],
            warnings: vec![],
            status: JobStatus::Ready,
            suffix_alias: None,
            source: None,
            pack: None,
            bake: None,
            rnm: None,
            derive: None,
            shader: None,
            cube: None,
            fill: None,
        };
        let texels = 48 * 40;
        assert_eq!(job_bytes(&job), input_bytes(48, 40));
        job.profile.mips.filter = MipFilter::Kaiser;
        assert_eq!(job_bytes(&job), input_bytes(48, 40) + texels * 8);
        job.profile.mips.filter = MipFilter::Box;
        job.profile.resize = ResizeMode::PotDown;
        assert_eq!(job_bytes(&job), input_bytes(48, 40) + texels * 32);

        use crate::dds::*;
        let dds = d.path().join("a.dds");
        let desc = DdsDesc {
            width: 48,
            height: 40,
            format: crate::config::Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        };
        let b = dds_bytes(&desc, &[vec![0; 48 * 40]]).unwrap();
        std::fs::write(&dds, &b).unwrap();
        job.input = dds;
        assert_eq!(
            job_bytes(&job),
            input_bytes(48, 40) + b.len() as u64 + texels * 32
        );
    }

    /// Carry 13, review M2/I2 and ruling O2: only a comparable, matching, passing record is up to date.
    #[test]
    fn up_to_date_needs_a_comparable_matching_passing_record() {
        use crate::manifest::MANIFEST_SCHEMA;
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("rock.png");
        std::fs::write(&src, b"source bytes").unwrap();
        let reg = PresetRegistry::builtin().unwrap();
        let mut job = ResolvedJob {
            input: src.clone(),
            output: d.path().join("rock.dds"),
            game_path: "rock".into(),
            set_key: "x/rock".into(),
            profile: reg.base_profile(Target::Cs, Subtype::Diffuse),
            provenance: Default::default(),
            suggestions: vec![],
            warnings: vec![],
            status: JobStatus::Ready,
            suffix_alias: None,
            source: None,
            pack: None,
            bake: None,
            rnm: None,
            derive: None,
            shader: None,
            cube: None,
            fill: None,
        };
        std::fs::write(&job.output, b"dds").unwrap();
        let enc = EncoderVersions::current();
        let entry_for = |job: &ResolvedJob| ManifestEntry {
            schema: MANIFEST_SCHEMA,
            output: "rock.dds".into(),
            source_sha256: sha256_file(&src).unwrap(),
            sources: vec![],
            profile_sha256: profile_sha256(&job.profile).unwrap(),
            engine_version: ENGINE_VERSION.into(),
            encode_version: Some(crate::manifest::encode_version(job.profile.subtype).into()),
            encoders: Some(enc.clone()),
            preset: PresetRef::for_preset(&reg, "cs-quality", None),
            bc1_model: Some("ideal".into()),
            output_stamp: SourceStamp::of(&job.output),
            copied_from: None,
            raw: serde_json::json!({ "validation": { "passed": true } }),
        };
        let up = |e: &ManifestEntry, job: &ResolvedJob, m: Bc1Model| {
            is_up_to_date(e, job, &enc, m, None)
        };
        let good = entry_for(&job);
        assert_eq!(job.profile.format, Format::Bc7);
        assert!(up(&good, &job, Bc1Model::Ideal));
        let with = |f: &dyn Fn(&mut ManifestEntry)| {
            let mut e = good.clone();
            f(&mut e);
            e
        };
        let not_up: [(&str, ManifestEntry); 15] = [
            (
                "no encode version, engine 0.9.0",
                with(&|e| {
                    e.encode_version = None;
                    e.engine_version = "0.9.0".into()
                }),
            ),
            (
                "newer encode version",
                with(&|e| e.encode_version = Some("99.0.0".into())),
            ),
            (
                "older encode version",
                with(&|e| e.encode_version = Some("0.9.0".into())),
            ),
            (
                "empty encode version",
                with(&|e| e.encode_version = Some(String::new())),
            ),
            ("no encoders", with(&|e| e.encoders = None)),
            (
                "other encoders",
                with(&|e| e.encoders.as_mut().unwrap().ispc = "1.0.0".into()),
            ),
            ("no preset", with(&|e| e.preset = None)),
            ("newer schema", with(&|e| e.schema = MANIFEST_SCHEMA + 1)),
            (
                "validation failed",
                with(&|e| e.raw = serde_json::json!({ "validation": { "passed": false } })),
            ),
            ("no validation", with(&|e| e.raw = serde_json::json!({}))),
            (
                "other profile",
                with(&|e| e.profile_sha256 = "0".repeat(64)),
            ),
            ("other source", with(&|e| e.source_sha256 = "0".repeat(64))),
            ("no output stamp", with(&|e| e.output_stamp = None)),
            (
                "replaced output",
                with(&|e| e.output_stamp.as_mut().unwrap().size += 1),
            ),
            (
                "several sources",
                with(&|e| {
                    e.sources = vec![crate::manifest::SourceRef {
                        path: "a.png".into(),
                        sha256: "0".repeat(64),
                        role: None,
                        invert: false,
                        bake: None,
                        rnm: None,
                        derive: None,
                        derive_role: None,
                        cube_face: None,
                        fill: None,
                    }]
                }),
            ),
        ];
        for (what, e) in &not_up {
            assert!(!up(e, &job, Bc1Model::Ideal), "{what}");
        }
        // Spec: the engine version no longer decides; a matching encode version from another engine is current.
        assert!(up(
            &with(&|e| e.engine_version = "0.0.0".into()),
            &job,
            Bc1Model::Ideal
        ));
        // A 1.0.0 record has no encode_version: current exactly when the type's arm is still LEGACY_ENCODE_VERSION.
        let legacy = with(&|e| {
            e.encode_version = None;
            e.engine_version = "1.0.0".into()
        });
        assert_eq!(
            up(&legacy, &job, Bc1Model::Ideal),
            crate::manifest::encode_version(job.profile.subtype)
                == crate::manifest::LEGACY_ENCODE_VERSION
        );
        // The BC1 model only matters for formats rgbcx encodes against it.
        assert!(up(&with(&|e| e.bc1_model = None), &job, Bc1Model::Nvidia));
        for format in [Format::Bc1, Format::Bc3] {
            job.profile.format = format;
            let e = entry_for(&job);
            assert!(up(&e, &job, Bc1Model::Ideal), "{format:?}");
            assert!(!up(&e, &job, Bc1Model::Nvidia), "{format:?} other model");
            let mut old = e.clone();
            old.bc1_model = None;
            assert!(
                !up(&old, &job, Bc1Model::Ideal),
                "{format:?} unrecorded model"
            );
        }
        // The rgbcx BC4 patch only changes BC3/BC4/BC5 output; a record from before it stays current for the rest.
        for format in [
            Format::Bc1,
            Format::Bc3,
            Format::Bc4,
            Format::Bc5,
            Format::Bc6hUf16,
            Format::Bc7,
            Format::Rgba8,
        ] {
            job.profile.format = format;
            let mut old = entry_for(&job);
            old.encoders.as_mut().unwrap().rgbcx_patch = None;
            let stale = matches!(format, Format::Bc3 | Format::Bc4 | Format::Bc5);
            assert_eq!(
                up(&old, &job, Bc1Model::Ideal),
                !stale,
                "{format:?} pre-patch record"
            );
        }
        // M5 (DECISIONS Note NV10): a record written before M5 has no `nvtt` key and stays current for built-in
        // profiles, whatever NVTT is found now.
        let pre_m5: EncoderVersions = serde_json::from_value(serde_json::json!({
            "bc7enc_rdo": enc.bc7enc_rdo, "ispctc": enc.ispctc, "ispc": enc.ispc,
            "bc7e_patch": enc.bc7e_patch, "rgbcx_patch": enc.rgbcx_patch,
        }))
        .unwrap();
        assert_eq!(pre_m5.nvtt, None);
        job.profile.format = Format::Bc7;
        let built_in = with(&|e| e.encoders = Some(pre_m5.clone()));
        assert!(is_up_to_date(
            &built_in,
            &job,
            &enc,
            Bc1Model::Ideal,
            Some("3.3.0")
        ));
        // An NVTT output is current only while the NVTT found now is the version that encoded it.
        job.profile.encoder.backend = Backend::Nvtt;
        let nv = |v: Option<&str>| {
            let mut e = entry_for(&job);
            e.encoders.as_mut().unwrap().nvtt = v.map(String::from);
            e
        };
        assert!(is_up_to_date(
            &nv(Some("3.2.5")),
            &job,
            &enc,
            Bc1Model::Ideal,
            Some("3.2.5")
        ));
        assert!(!is_up_to_date(
            &nv(Some("3.2.5")),
            &job,
            &enc,
            Bc1Model::Ideal,
            Some("3.3.0")
        ));
        assert!(!is_up_to_date(
            &nv(Some("3.2.5")),
            &job,
            &enc,
            Bc1Model::Ideal,
            None
        ));
        assert!(!is_up_to_date(
            &nv(None),
            &job,
            &enc,
            Bc1Model::Ideal,
            Some("3.2.5")
        ));
    }

    #[test]
    fn encode_current_compares_the_record_with_the_given_type_version() {
        let mk = |enc: Option<&str>, engine: &str| ManifestEntry {
            schema: crate::manifest::MANIFEST_SCHEMA,
            output: "x.dds".into(),
            source_sha256: String::new(),
            sources: vec![],
            profile_sha256: String::new(),
            engine_version: engine.into(),
            encode_version: enc.map(String::from),
            encoders: None,
            preset: None,
            bc1_model: None,
            output_stamp: None,
            copied_from: None,
            raw: serde_json::json!({}),
        };
        let e = mk(Some("1.0.0"), "1.1.0");
        assert!(encode_current(&e, "1.0.0")); // type unchanged in 1.1.0
        assert!(!encode_current(&e, "1.1.0")); // type bumped in 1.1.0
        let legacy = mk(None, "1.0.0");
        assert!(encode_current(&legacy, "1.0.0"));
        assert!(!encode_current(&legacy, "1.1.0"));
        assert!(!encode_current(&mk(None, "0.9.0"), "1.0.0"));
    }

    /// M1: with bytes to spare, the file cap alone holds the next admission until a file finishes.
    #[test]
    fn budget_caps_unfinished_files() {
        let b = Budget::new(u64::MAX, 2);
        b.acquire(1);
        b.acquire(1);
        let admitted = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                b.acquire(1);
                admitted.store(true, Ordering::SeqCst);
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(
                !admitted.load(Ordering::SeqCst),
                "third file admitted past the cap"
            );
            b.release(1);
        });
        assert!(admitted.load(Ordering::SeqCst));
        assert_eq!(*lock(&b.used), (2, 2));
    }

    #[test]
    fn target_origin_follows_precedence() {
        let reg = PresetRegistry::builtin().unwrap();
        let chain = reg.chain("vanilla-quality").unwrap();
        let mut req = BatchRequest::new(vec![], "out".into(), Target::Cs);
        let cfg = |t| ConfigFile {
            target: Some(t),
            ..Default::default()
        };
        let (proj, user) = (cfg(Target::Vanilla), cfg(Target::Cs));
        assert_eq!(
            resolve_target(&req, Some(&proj), Some(&user), &chain),
            (Target::Cs, Provenance::User)
        );
        req.target = None;
        assert_eq!(
            resolve_target(&req, Some(&proj), Some(&user), &chain),
            (Target::Vanilla, Provenance::ProjectConfig)
        );
        assert_eq!(
            resolve_target(&req, None, Some(&user), &chain),
            (Target::Cs, Provenance::UserConfig)
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
