//! The view-model (M3a spec §3): all GUI state and logic, no egui types, testable without a window.
pub mod also;
pub mod auto_flags;
mod auto_record;
pub mod bottom;
pub mod bulk;
pub mod checks;
pub mod cm_json;
pub mod code_text;
pub mod cs_pbr;
pub mod cs_pbr_export;
pub mod cube_cross;
pub mod dds_patch;
pub mod encoding_follow;
pub mod export;
pub mod fields;
pub mod from_normal;
pub mod heatmap;
pub mod helpers;
pub mod history;
pub mod input_name;
pub mod inspector;
pub mod items;
pub mod known_paths;
pub mod labels;
pub mod marker;
pub mod meshes;
pub mod messages;
pub mod nav;
pub mod out_size;
pub mod overlay;
pub mod pack_fills;
pub mod pbr_export;
mod pbr_follow;
pub mod picker;
pub mod pin;
pub mod plugin_refs;
pub mod presets;
pub mod preview;
pub mod readout;
mod reencode;
pub mod rename;
pub mod review;
pub mod rnm;
pub mod rnm_preview;
pub mod sections;
pub mod set_preview;
pub mod sets;
pub mod sets_filter;
pub mod shell;
pub mod skin;
pub mod texture_view;
pub mod used_by;
pub mod uv_coverage;
pub mod view;
pub mod viewport3d;

use crate::project::{ProjectDoc, SaveError};
use crate::settings::GuiSettings;
use crate::worker::{
    spawn_dds_thumbs, spawn_export, spawn_guarded, spawn_plan, MeshEvidence, PlanFailure, WorkerMsg,
};
use engine::api::{BatchRequest, CancelToken};
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype, Target};
use engine::policy::{AcceptSuggestions, SourceStamp};
use engine::presets::ConfigFile;
use engine::session::{Session, Thumbnail};
use items::*;
use serde_json::json;
use std::path::PathBuf;

/// The file an NVTT probe ran on: path, size and modified time (`AppModel::refresh_nvtt_if_changed`).
type NvttKey = (PathBuf, u64, Option<std::time::SystemTime>);
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Note FC11 (supersedes P21's note): a hash recorded while `sdc-project.toml` is read-only.
pub const SIDECAR_ONLY: &str =
    "hash recorded in _sdc/generated.json only: sdc-project.toml is read-only";

/// Note FC14: the note for a hash recorded while `sdc-project.toml` is read-only, from the sidecar write's result.
fn sidecar_only_note(written: &std::io::Result<()>) -> String {
    match written {
        Ok(()) => SIDECAR_ONLY.into(),
        Err(e) => format!("hash not recorded: {e}"),
    }
}

/// How long the "Undid/Redid" note stays in the status line (AG15).
const UNDO_NOTE_FOR: Duration = Duration::from_secs(5);

/// Whether a warning counts toward the amber ⚠ badge and the "Has warnings" filter. Info lines (a note, not a
/// problem) stay in the texture's own warnings list only. One predicate for the pill, the tile and the filter.
pub fn counts_toward_badge(w: &engine::codes::Warning) -> bool {
    use engine::codes::Severity;
    matches!(
        w.severity,
        Severity::Warning | Severity::Error | Severity::Blocking
    )
}

/// Item 13: the plan bar's text, "Analysing 1,234 / 5,000 files…".
pub fn plan_bar_text(done: usize, total: usize) -> String {
    format!(
        "Analysing {} / {} files…",
        thousands(done),
        thousands(total)
    )
}

/// Note FC12: what the top undo step's label gets when the engine rejected `sdc.toml` after it (then the message, `)`).
const REJECTED: &str = " (sdc.toml rejected it: ";

/// `n` with a comma between each group of three digits.
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Note FB7: what the centre column shows: the selection (a texture, a set, the preview, the start screen), or the
/// docked Presets or TruePBR JSON view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CentreTab {
    #[default]
    Selection,
    Presets,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Item(String),
    Set(String),
}

pub use texture_view::Filter;

/// (sdc.toml rev, dirty, only automatic types unsaved), (sdc-project.toml rev, dirty, read-only).
type RecoveryKey = (Option<(u64, bool, bool)>, Option<(u64, bool, bool)>);

/// Which of the two project files a dialog or save error is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    SdcToml,
    Project,
}

impl DocKind {
    pub fn file(self) -> &'static str {
        match self {
            DocKind::SdcToml => "sdc.toml",
            DocKind::Project => "sdc-project.toml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    ExternalChange(DocKind),
    CloseUnsaved,
    /// Opening this folder would drop unsaved edits to the current `sdc.toml`.
    OpenUnsaved(PathBuf),
    /// The banner's Reload would drop unsaved edits and undo (trace-debug #10).
    ReloadUnsaved,
    /// An export is running; closing or opening would have to cancel it first (M3a spec §2.6).
    ExportRunning(AfterExport),
    /// F3a D4: copy the ticked `[files]` fields of texture `from` to `to` (the other selected textures).
    CopySettings {
        from: String,
        to: Vec<String>,
        fields: Vec<(String, bool)>,
    },
    /// F3a D4: remove the `[files]` settings of these textures, once confirmed.
    ConfirmResetAll(Vec<String>),
    /// F3b: the rename dialog (input aliases or output names).
    Rename(rename::RenameDialog),
    /// v2 Track B: "Generate from normal map…" for this set id; its state is `AppModel::gen` (floats are not `Eq`).
    GenerateFromNormal(String),
    /// Note CO: "Delete generated file…", asked once per file; texts and refusals read when it opened.
    ConfirmDeleteGenerated(Box<from_normal::DeleteGenerated>),
    /// Export to ▾: Make marker cube… (cubemap GUI, Task 6); the draft is AppModel::marker.
    MakeMarker,
    Error(String),
}

/// How the export continues once the plan in flight has landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Continue {
    /// R3: Review's Export: the guards and the Review gate again, then the export (undecided textures skipped).
    Review,
}

/// What was asked while an export ran: it happens when the export has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterExport {
    Close,
    Open(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Subtype(Subtype),
    Alpha(AlphaPolicy),
    Green(GreenConvention),
    Acknowledge(Code),
}

/// What a plan takes from the sets: `set_edits`, `output_paths`, and the shared roughness slots and borrowed
/// normals that gate the bake (DECISIONS R7, Note R M4-final-1). All four settle the two-pass planning.
#[derive(Debug, Clone, PartialEq)]
struct EngineInputs {
    set_edits: Vec<engine::sets::SetEdit>,
    output_paths: std::collections::BTreeMap<engine::sets::OutputKey, PathBuf>,
    shared_roughness: std::collections::BTreeSet<String>,
    borrowed_normal: std::collections::BTreeMap<String, String>,
    set_shaders: std::collections::BTreeMap<String, engine::shader::Shader>,
    pinned_files: std::collections::BTreeSet<String>,
    mesh_votes: std::collections::BTreeMap<String, Vec<engine::shader::Shader>>,
    mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
    gras_diffuses: std::collections::BTreeSet<String>,
    gras_all_diffuses: std::collections::BTreeSet<String>,
    cs_txst_sets: std::collections::BTreeSet<String>,
}

impl EngineInputs {
    fn of(m: &AppModel) -> Self {
        let (set_edits, output_paths) = sets::engine_inputs(m);
        EngineInputs {
            set_edits,
            output_paths,
            shared_roughness: sets::shared_roughness(m),
            borrowed_normal: sets::borrowed_normal(m),
            set_shaders: sets::set_shaders(m),
            pinned_files: sets::pinned_files(m),
            mesh_votes: m.mesh_votes.clone(),
            mesh_alpha: m.mesh_alpha.clone(),
            gras_diffuses: m.gras_diffuses.clone(),
            gras_all_diffuses: m.gras_all_diffuses.clone(),
            cs_txst_sets: sets::cs_txst_sets(m),
        }
    }
}

fn read_only_banner(error: &str) -> String {
    format!("{error} — sdc.toml is ignored and the GUI is read-only until it is fixed and reloaded")
}

/// `(evidence_rev, export.revision, export running)`.
type ProblemsKey = (u64, u64, bool, u64);

/// The cached set views, export blocks and delete-section hits, valid for `key` only.
#[derive(Default)]
pub(crate) struct ViewCache {
    pub key: Option<ViewKey>,
    pub views: Option<Arc<Vec<sets::SetView>>>,
    pub blocks: Option<Arc<Vec<pbr_export::ExportIssue>>>,
    pub hits: Option<Arc<Vec<sections::DeleteHit>>>,
    /// Keyed by (evidence_rev, export.revision, export running) inside the view_key cell (K9).
    pub problems: Option<(ProblemsKey, Arc<Vec<bottom::Problem>>)>,
    /// `export_asks`, keyed by `evidence_rev` inside the view_key cell (R-b: Review reads them every frame).
    pub asks: Option<(u64, Arc<Vec<pbr_export::ExportIssue>>)>,
    /// `AppModel::export_files`, under the same key and `inner` tuple as `problems` (Task 6; ui-rules §9.1, §7.7).
    pub export_files: Option<(ProblemsKey, usize)>,
    /// `AppModel::half_size_rows`, under the same key and `inner` tuple (Note FC2).
    pub half_size: Option<(ProblemsKey, Arc<Vec<review::HalfSizeRow>>)>,
    /// The keys `bottom::set_ready` looks up; reset with every `problems` build.
    pub flagged: Option<Arc<bottom::Flagged>>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ViewKey {
    items: u64,
    inputs: u64,
    doc: u64,
    root: Option<PathBuf>,
    export_root: PathBuf,
    target: Target,
    /// The scan database the statics rule read (design §5.1): a new scan changes the export blocks.
    scan_db: Option<PathBuf>,
    /// Its stamp as of the last `poll` (an external CLI scan shows on the next frame).
    scan: cs_pbr::Stamp,
    /// Note FC12: "Also write a zip" is in effect, so the zip-size row follows the tick on the next frame.
    zip: bool,
}

/// A click or key's action that waits a frame for a text field to commit (Note FC8).
pub type AfterCommit = Box<dyn FnOnce(&mut AppModel)>;

pub struct AppModel {
    /// The file pickers (Task 6); one dialog at a time, shared with the sub-models.
    pub picker: picker::Picker,
    pub settings: GuiSettings,
    pub settings_path: Option<PathBuf>,
    /// gui.toml as last loaded or written, and its stamp then (audit item 15); `None`: no merge on the next save.
    settings_base: Option<(GuiSettings, Option<SourceStamp>)>,
    /// The last gui.toml write error shown (audit item 9: each distinct error is toasted once).
    settings_error: Option<String>,
    /// What the last recovery snapshot was taken at (audit item 11).
    recovery_key: Option<RecoveryKey>,
    /// Note FB5: the restore step on the undo stack (its depth and the `history.pushes` it left, which identify it)
    /// and the files it restored, for the toast's Undo.
    crash_restore: Option<(usize, u64, Vec<PathBuf>)>,
    /// Documents that were read-only while a recovery file waited: once fixed and reloaded, the file's age no longer
    /// counts (fixing the document made it newer than the file).
    // shortcut: kept in memory only. Quit, fix the file by hand and relaunch, and the older `.recovery` file is
    // neither restored nor named; persist the marker beside the file if users report orphaned `.recovery` files.
    held_recovery: std::collections::BTreeSet<PathBuf>,
    /// Bumped by `open_folder` and a new export: the drawer then forgets its cached file checks.
    pub drawer_epoch: u64,
    /// Bumped by `open_folder`: the UI then drops its Sets column modal and Slot commands (trace-debug #52).
    pub folder_epoch: u64,
    /// `%APPDATA%\sdc\config.toml`, set by `main`; `None` in tests so a developer's own config cannot leak in.
    pub user_config: Option<PathBuf>,
    /// `%APPDATA%\sdc\presets`, set by `main`; `None` in tests so a developer's own presets cannot leak in.
    pub user_presets: Option<PathBuf>,
    /// `(id, file)` of the user presets the last plan loaded (the preset inspector lists them).
    pub loaded_user_presets: Vec<(String, String)>,
    /// `(file, reason)` of the user preset files the last plan skipped (CFG-PRESET-SKIPPED; the inspector lists them).
    pub skipped_user_presets: Vec<(String, String)>,
    /// Note FC3 (Y4): the last plan's IN-UNREADABLE warnings, one Problems row each.
    pub unreadable: Vec<engine::codes::Warning>,
    /// FC7: what the last plan re-encodes after an upgrade, one Review Warning row per cause (`FirstExport` a line).
    /// Emptied when an export starts.
    pub recode: Vec<engine::manifest::RecodeCause>,
    /// FC11: the recorded files the last plan no longer writes, Review's `Stale outputs (n)`. Not a gate.
    pub stale: Vec<engine::manifest::StaleOutput>,
    /// FC11 (fix round 1): the JSON rows of `stale`, each with the SHA-256 its hash record holds (their baseline).
    pub stale_json: std::collections::BTreeMap<PathBuf, String>,
    /// Note FC3: the last Also export's conversion report, the DERIVE rows' Open report.
    pub also_report: Option<PathBuf>,
    /// Note FC10 (CN43): the PBR folder's `_sdc/also.json` that the last Also export could not read.
    pub also_unreadable: Option<PathBuf>,
    /// Note FC10 (item 27): the project files with the read-only flag, as of the last open, reload or save.
    pub read_only_flags: Vec<PathBuf>,
    /// Note FC3: the NIF edit's problems, handed over by `app.rs` each frame (`set_nif_problems`).
    pub nif_problems: Arc<Vec<meshes::NifProblem>>,
    /// The scan database the mesh votes read (`meshes::default_store_path()`), set by `main`; `None` in tests so a
    /// developer's scan cannot leak in.
    pub scan_db: Option<PathBuf>,
    /// Trace-debug wave 2 #6: a Meshes scan is running, or the last one was cancelled or failed, so the store has every
    /// origin disabled and reads empty; the mesh evidence and "Used by meshes" keep what they have. Set by the app.
    pub scan_store_unsettled: bool,
    /// What `scan_store_unsettled` was when the running scan started (Note CW2: an error raised before `begin_scan` restores it).
    unsettled_before_scan: bool,
    /// Note FC13 (CL32 M6): a Meshes scan is running now, between `scan_started` and `scan_finished`; with
    /// `scan_store_unsettled` and no run, the last scan was stopped. Read by the UV coverage reasons.
    pub scan_running: bool,
    /// `scan_db`'s stamp, taken once per frame in `poll` (REPORT #9: `view_key` does not stat it per call).
    scan_stamp: cs_pbr::Stamp,
    /// The last scan-database read error logged (audit GM-5: each distinct error once).
    scan_db_error: Option<String>,
    /// S-shader-10: `shader::mesh_key` → what the scanned meshes report; sent as `BatchRequest::mesh_shaders`.
    pub mesh_votes: std::collections::BTreeMap<String, Vec<engine::shader::Shader>>,
    /// M6 Track E (DECISIONS Note U5): `shader::colour_mesh_key` → the alpha property of each scanned shape that uses
    /// the colour member; sent as `BatchRequest::mesh_alpha`.
    pub mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
    /// Scan evidence for the Subsurface auto mode (DECISIONS AG102): `shader::colour_mesh_key` → whether any scanned
    /// source shape that uses the colour member is Tree_Anim (SLSF2 bit 29). Not sent to the engine. Written only
    /// through `set_tree_anim`: the cached `export_asks` is keyed on the `evidence_rev` it bumps.
    pub mesh_tree_anim: std::collections::BTreeMap<String, bool>,
    /// Bumped whenever `mesh_tree_anim` or `known` is replaced (DECISIONS AG109); part of `json_key`.
    pub evidence_rev: u64,
    /// Complex grass design §5: the GRAS diffuses of the scan (`engine::grass::path_key` keys), read with the mesh
    /// evidence; sent as `BatchRequest::gras_diffuses`.
    pub gras_diffuses: std::collections::BTreeSet<String>,
    /// PBR grass design §4.2: every GRAS diffuse of the scan, TruePBR shapes kept, read with the mesh evidence; sent
    /// as `BatchRequest::gras_all_diffuses`.
    pub gras_all_diffuses: std::collections::BTreeSet<String>,
    /// GUI tweaks G1/G5: the texture and mesh paths of the last scan, for the path pickers. Written only through
    /// `set_known`: the cached `export_asks` is keyed on the `evidence_rev` it bumps.
    pub known: Arc<known_paths::KnownPaths>,
    /// Item 9: the selected texture's "Used by meshes" read (`used_by_for`).
    pub used_by: used_by::UsedByState,
    /// Note AN: the hover lists of PGPatcher-locked slots.
    pub original: used_by::OriginalState,
    /// G3: the TruePBR JSON editor is open (Note FB7: a centre tab, or its own window when popped out).
    pub json_editor_open: bool,
    /// Note AM: the preset creator is open (Note FB7: a centre tab, or its own window when popped out).
    pub presets_open: bool,
    /// Note FB7: the view the centre column shows in front (`centre_front` checks it is open and docked).
    pub centre_tab: CentreTab,
    /// Note FB7: Pop out shows the view in an OS window of its own; Dock brings it back to the centre.
    pub presets_popped: bool,
    pub json_editor_popped: bool,
    /// Note FB7: a view was opened docked this frame; the Meshes view, which has no centre column, switches to
    /// Textures to show it.
    pub reveal_centre: bool,
    /// Note AM: the presets folder as last scanned, and the creator's draft; made on first use (`presets_model`).
    pub presets: Option<presets::PresetsModel>,
    /// What the plan accepts (M6 Track E spec §2, DECISIONS Note U3): High suggestions, as the app runs. The M3-era test
    /// fixtures set `None` to keep their High members undecided, as the CLI does without the flag.
    pub accept: AcceptSuggestions,
    pub session: Option<Arc<Session>>,
    pub root: Option<PathBuf>,
    pub project: Option<ProjectDoc>,
    pub sets_doc: Option<crate::sets_doc::SetsDoc>,
    pub history: history::History,
    pub target: Target,
    pub items: Vec<TextureItem>,
    pub sets: Vec<SetItem>,
    /// The export folder `sets` were planned with (audit 08c-3): set ids strip it, also while a re-plan for a new
    /// export folder runs. `None` before the first plan.
    sets_root: Option<PathBuf>,
    /// Bumped whenever `items` is replaced (or announced with `invalidate_views`): the set-view cache key.
    items_rev: u64,
    /// Note FI15: the opened folder's output game paths and their revision (see `output_game_paths`).
    game_paths: (u64, Vec<String>),
    /// Note FI1: skipped DDS inputs' tile images by path, with the stamp they were decoded at; cleared on open and
    /// reload.
    dds_thumbs: std::collections::HashMap<PathBuf, (SourceStamp, Arc<Thumbnail>)>,
    /// The DDS tile images asked for and not landed yet (no second request while one is out; the repaint gate).
    dds_thumbs_asked: std::collections::HashMap<PathBuf, SourceStamp>,
    /// DDS tile images the decoder refused, with the stamp it refused (not read again until the file changes).
    dds_thumbs_failed: std::collections::HashMap<PathBuf, SourceStamp>,
    /// Paths sent to `spawn_dds_thumbs` so far (tests: a re-plan reuses the cache).
    #[doc(hidden)]
    pub dds_thumb_requests: usize,
    /// The artist check results and what they were computed from (DECISIONS Note CL).
    pub(crate) check_state: checks::CheckState,
    /// Bumped whenever `last_inputs` is written: `engine_inputs` (so the export blocks) reads it.
    inputs_rev: u64,
    /// `TextureItem::key` to its index in `items` (audit 7).
    item_index: std::collections::HashMap<String, usize>,
    /// The cached set views and export blocks (DECISIONS Note Y7).
    pub(crate) views: std::cell::RefCell<ViewCache>,
    /// K3: `review_json` for a view key, and when it was read; cleared with `review`.
    review_json_cache: std::cell::RefCell<Option<(ViewKey, Instant, Vec<review::JsonLine>)>>,
    /// The rename dialog's last plan, for drawing only (Note FA).
    rename_cache: std::cell::RefCell<Option<rename::PlanCache>>,
    pub unmatched_keys: Vec<String>,
    /// Per-set RNM-* notes from layer jobs (they have no item).
    pub rnm_notes: std::collections::BTreeMap<String, Vec<engine::codes::Warning>>,
    /// Layer, mask and AO path to its `[files]` key.
    pub rnm_keys: std::collections::BTreeMap<PathBuf, String>,
    /// The preset id the last plan resolved (empty before the first plan).
    pub batch_preset: String,
    /// While set, `edit_docs` changes the documents but neither records a step nor re-plans.
    batch: bool,
    pub selection: Selection,
    /// F3a D1: the multi-selection; `selection` stays the active entry.
    pub multi: bulk::MultiSelection,
    /// F3a: the bulk panel's chip and last report.
    pub bulk: bulk::BulkState,
    pub filter: Filter,
    /// The Textures column's sort and tile size (item 8).
    pub prefs: texture_view::ColumnPrefs,
    pub(crate) texture_cache: std::cell::RefCell<texture_view::TextureViewCache>,
    pub search: String,
    /// The Sets column's search and filters (layout v2, R17).
    pub sets_filter: sets_filter::SetsFilter,
    /// Note FC13 (CD16): the filter that was on when sets were created, and their ids; shown until it changes.
    pub(crate) new_sets: Option<(sets_filter::SetsFilter, Vec<String>)>,
    pub(crate) sets_filter_cache: std::cell::RefCell<sets_filter::SetsFilterCache>,
    pub status_line: String,
    /// Item 20: the toasts and the Messages log (DECISIONS AG112-AG114); every status-line note also lands here.
    pub messages: messages::Messages,
    /// The bottom panel: open or collapsed, and its tab (layout v2 A1).
    pub bottom: bottom::BottomPanel,
    /// Review (R1): open while `Some`; it holds only what the user answered.
    pub review: Option<review::Review>,
    pub(crate) problem_builds: std::cell::Cell<u64>,
    pub(crate) filter_builds: std::cell::Cell<u64>,
    /// Task 6: `bottom::problems` calls and `export_files` builds (see `bottom::problem_lookups`).
    pub(crate) problem_lookups: std::cell::Cell<u64>,
    pub(crate) export_files_builds: std::cell::Cell<u64>,
    /// Task 5: set cards laid out (`sets_column::pill` calls); see `sets_filter::set_cards_drawn`.
    pub(crate) set_cards_drawn: std::cell::Cell<u64>,
    /// Item 22: per-item DDS sizes by plan revision.
    pub sizes: out_size::SizeCache,
    /// Item 21: the RNM blended thumbnail job.
    pub rnm_preview: rnm_preview::RnmPreview,
    /// A note kept in front of the status line until the next Export click (an Export cancelled by an edit, AF4).
    pub status_note: Option<String>,
    /// "Undid: <label>" / "Redid: <label>" in front of the status line until the next edit (item 2).
    undo_note: Option<String>,
    /// When the note was set, and whether a plan has shown it: it goes with the next plan after that, or after 5 s.
    undo_note_at: Option<(Instant, bool)>,
    /// AM17: a banner `refresh_banner` does not build (the startup "Settings were not read"): kept on every rebuild
    /// until dismissed.
    pub kept_banner: Option<String>,
    pub banner: Option<String>,
    /// The lines of the last banner, so a rebuild logs only a line that is new (Note FB8).
    banner_logged: Vec<String>,
    /// Task 9 (Z11): the last "encoding follows type" cleanup, as (status-line text, hover listing every texture).
    pub encoding_note: Option<(String, String)>,
    /// Per packed output key: fills a recipe switch removed because the new recipe has no such channel.
    pub dropped_fills: std::collections::BTreeMap<String, String>,
    /// Note FC3: `history.pushes` when `dropped_fills` and `encoding_note` were written: their rows offer Undo
    /// while it still is.
    pub(crate) dropped_fills_step: u64,
    pub(crate) encoding_note_step: u64,
    pub dialog: Option<Dialog>,
    /// The Make marker cube dialog's colour and roughness.
    pub marker: marker::MarkerDraft,
    /// The last kept or failed marker write's note, shown inside the dialog (DT34); cleared when it opens.
    pub marker_note: Option<(messages::Severity, String)>,
    /// F3b D4: the rename dialog's last applied recipe, offered again on the next open (this session only).
    pub last_recipe: rename::Recipe,
    pub expanded: Option<String>,
    pub export: export::ExportModel,
    pub preview: preview::PreviewModel,
    /// The DDS header-patch choice (DECISIONS S7).
    pub dds_patch: dds_patch::DdsPatchState,
    pub view: view::ViewState,
    /// The preview overlay job and the overlay on show (v2 checks, Note CL).
    pub overlay: overlay::OverlayState,
    /// UV coverage: the session cache, its reads, Check all and the scan-has-rows answer (Note CL, Task 8).
    pub uv: uv_coverage::UvState,
    /// The largest texture side the GPU takes; set from the renderer at start-up (M3a spec §6.5).
    pub gpu_max_dim: u32,
    /// False when eframe found no usable GPU adapter: the preview is off, everything else works (M3a spec §7).
    pub gpu_available: bool,
    /// The BC1 decoder choice differs from the session's model, which is fixed at launch (M3a spec §4.6).
    pub restart_needed: bool,
    /// M5 (DECISIONS Note NV3): the NVTT probe of `settings.nvtt_path`, run on start, on a path change and on window
    /// focus; `None` until the first run.
    pub nvtt: Option<engine::nvtt::NvttProbe>,
    /// Generation of the latest NVTT probe, and whether it still runs (on a worker, DECISIONS AF3).
    nvtt_gen: u64,
    nvtt_pending: bool,
    /// The file the last probe ran on (path, size, modified time; `None` inside: none found), `None` before any.
    nvtt_key: Option<Option<NvttKey>>,
    /// The set editor "Used by plugins" reads (esp spec 4.1).
    pub plugin_refs: plugin_refs::SetPluginRefs,
    /// Design §5.1, §6: the CS PBR scan reads (statics, EDID checks, the material-object picker).
    pub cs_pbr: cs_pbr::CsPbrScan,
    /// Keyboard navigation, the shortcut sheet and the command palette (items 5 and 19).
    pub nav: nav::Nav,
    /// Explorer requests the app has not started yet (Task 5, `model::shell`).
    pub shell: Vec<shell::ShellRequest>,
    /// File-exists answers asked while drawing (the scan DB, Show in Explorer), re-checked at most once a second
    /// (AG128).
    pub stats: std::cell::RefCell<shell::StatCache>,
    /// Header sizes read for the Generate offers, by (path, size, mtime) (Note CO, spec-M5).
    pub(crate) gen_dims: std::cell::RefCell<from_normal::GenDims>,
    /// Generated-file facts for the slot rows: the normals' hashes by (path, stamp) and `GenInfo` by item (Task 7).
    pub(crate) gen_info: std::cell::RefCell<from_normal::GenInfoCache>,
    /// The Generate path's alias map by plan, and its lookup counters (Task 10, Task 6 review M6).
    pub(crate) gen_look: std::cell::RefCell<from_normal::GenLookups>,
    /// The Generate dialog's state while it is open, or while a cancelled Generate that started writing lands.
    pub gen: Option<from_normal::GenDialog>,
    /// The first file the last Generate wrote: selected by the plan that lists it.
    pub gen_select: Option<String>,
    /// Note FC8: clicks and keys that waited one frame for a focused text field to commit (`ui::after_commit`); run
    /// first thing next frame, by `ui::actions::keys`.
    pub after_commit: Vec<AfterCommit>,
    /// Note FC8 (CP10): a BC7 weighting pick whose own value may have hidden an inherited one: (item key, pick, the
    /// pick's history merge key). The settled plan checks it and completes the pick in the same step.
    weighting_follow: Option<(String, String, String)>,
    /// Note FI3: a re-encode step's history merge key; the FC1 recording its settled plan makes joins that step.
    reencode_follow: Option<String>,
    /// The last Generate job id, so a new dialog's ids never repeat a closed one's.
    pub(crate) gen_ids: u64,
    /// Generate dialogs opened so far; the preview zoom is reset when this changes.
    pub(crate) gen_opens: u64,
    /// A Save report write is on a worker (AG130); the app keeps frames coming until it lands.
    pub report_saving: bool,
    /// "Show in Meshes" asked for this texture (normalised scan path); app.rs switches the view (Note GC).
    pub show_in_meshes: Option<String>,
    /// A NIF worklist row's fix: (NIF path, tab, field); app.rs switches to the Meshes view (NIF team, Note GD1).
    pub nif_tab_request: Option<(String, meshes::NifTab, Option<nif::fields::Field>)>,
    /// Note FC1 (AM14): `Choose preset…` was clicked on this pass (`ctx.cumulative_pass_nr`); the header opens its
    /// Preset menu if it draws within two passes, and drops a stale request.
    pub choose_preset: Option<u64>,
    /// Note FC1: the last automatic recording: its row text, its lines (the row's hover) and the `history.pushes`
    /// it left.
    pub(crate) auto_note: Option<auto_record::AutoNote>,
    /// Note FC1 (AM14): `preset` names no installed preset; computed once per plan request.
    missing_preset_id: Option<String>,
    /// Export was clicked while sources had changed: it starts when the fresh plan arrives.
    export_after_plan: bool,
    /// Review's Export was clicked while a plan or scan read was in flight: the export continues when it lands.
    continue_after_plan: Option<Continue>,
    /// The user chose to cancel the running export and then close or open: done when the export has ended.
    after_export: Option<AfterExport>,
    /// What the running export writes afterwards (the JSON and the vanilla copies).
    pending_json: Option<pbr_export::PendingJson>,
    /// Design §5: the CS PBR files the running export writes afterwards.
    pending_cs: Option<cs_pbr_export::PendingCs>,
    /// Complex material design §6: the `<texture>.json` files the running export writes afterwards.
    pending_cm: Option<cm_json::PendingCm>,
    /// C4: Review's answer for the edited PGPatcher JSON, for the running export only (K12).
    pending_json_answer: Option<review::JsonAnswer>,
    /// Note FC5: the NIF edits Review lists, as `app.rs` hands them over each frame (`MeshesModel::nif_review_rows`).
    pub(crate) nif_rows: Vec<meshes::NifReviewRow>,
    /// Note FC5: Review's Keep/Overwrite for a foreign NIF, for `app.rs` to pass to `MeshesModel::set_nif_answer`.
    pub nif_answer_request: Option<(String, meshes::export::GuardChoice, [u8; 32])>,
    /// Note FC5 (GD29): Review closed or the project changed; `app.rs` calls `MeshesModel::clear_nif_answers`.
    pub nif_clear_answers_request: bool,
    /// Note FC5 (GD2): the NIF footer's `Review…`; `app.rs` sets it and `poll` opens Review, in the view on show (GF1).
    pub open_review_request: bool,
    /// Note GF2: a foreign NIF row's `Diff…` in Review (its data path); `app.rs` calls `MeshesModel::show_nif_diff`.
    pub nif_diff_request: Option<String>,
    /// Note FC5: the running export was started by Review's Export and not cancelled: the NIF edits export after it.
    nif_export_queued: bool,
    /// Note FC5: the run has ended; `take_nif_export_due` hands it to `app.rs` once.
    nif_export_due: bool,
    /// Note CN: the running (or last) export is an Also export (`run_also`).
    also_pass: bool,
    /// Note CN: the last Also export's phase error (CN56), so its end is logged as an error, not a success.
    also_error: Option<String>,
    /// Note CN: the three folders the last Also export wrote (`also_file`).
    also_written: Option<[PathBuf; 3]>,
    /// Final review I2: the Also worker waits on this for the files the JSON finishers wrote into the PBR folder.
    also_reply: Option<Sender<Vec<PathBuf>>>,
    /// Final review I2: the files the JSON finishers wrote (or found current) in this export.
    also_json: Vec<PathBuf>,
    /// Note FC12: the zip-size row's limits (entries, bytes): the ZIP format's, lowered only by tests.
    zip_limits: (usize, u64),
    /// Final review I2: the token `export_ended` took, re-armed while the walk, zips and report run.
    also_cancel: Option<CancelToken>,
    /// Final review I2: `Some` while the walk, zips and report run: the export's failure, shown when they end.
    also_tail: Option<Option<String>>,
    /// `sdc.toml` was made read-only because the engine rejected it (not because it failed to parse).
    ignored_by_plan: bool,
    /// Note FC14: `history.pushes` at the last `replan`, and, when a step was pushed since the one before, that count:
    /// the step whose edit started this plan, the only one a rejected `sdc.toml` blames.
    replan_pushes: (u64, Option<u64>),
    /// The export that was in the way of a close has ended; the window may close now (see `take_close_ready`).
    close_ready: bool,
    /// The engine inputs the latest plan was requested with (DECISIONS P1: two-pass planning).
    last_inputs: Option<EngineInputs>,
    /// Automatic re-plans in a row because the inputs changed; bounded so a bug can never loop.
    auto_replans: u8,
    /// Task 9 (Z11): plans of an undo, redo, open or reload do not clean up encodings; the next settled plan ends it.
    follow_off: bool,
    /// A cleanup waiting for the plan that shows its new defaults (the note is written then).
    type_pending: Option<encoding_follow::Pending>,
    /// The sub-types before the plan chain in flight (`settle_types` compares them with the final pass).
    type_base: Option<std::collections::HashMap<String, Subtype>>,
    /// A user step started the chain in flight: its cleanup folds into that step.
    type_cause: bool,
    /// One-shot guard: the chain in flight is a cleanup's own replan.
    type_cleaned: bool,
    /// Generation of the latest scan-database read, and (generation, continues `apply_plan`) while it is pending.
    votes_gen: u64,
    votes_pending: Option<(u64, bool)>,
    generation: u64,
    planning: Option<CancelToken>,
    /// Item 13: (done, total) of the plan in flight, from `PlanProgress`; `None` once it lands.
    pub plan_progress: Option<(usize, usize)>,
    /// While a folder's first plan is in flight: the folder open before it (`None`: none), Recent as it was, and that
    /// folder's undo history and selection (Note FC12). Cancel goes back to all of them.
    opening: Option<(Option<PathBuf>, Vec<PathBuf>, history::History, Selection)>,
    /// The last open was cancelled with no folder to go back to (the empty state says so).
    open_cancelled: bool,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl AppModel {
    pub fn new(
        settings: GuiSettings,
        settings_path: Option<PathBuf>,
        session: Arc<Session>,
    ) -> AppModel {
        let mut m = AppModel::placeholder(settings);
        m.settings_base = settings_path
            .as_deref()
            // #45: no stamp when the file could not be read (locked at launch), so the first save merges it.
            .map(|p| {
                let read = GuiSettings::load(p).1.is_none();
                (m.settings.clone(), SourceStamp::of(p).filter(|_| read))
            });
        m.settings_path = settings_path;
        m.session = Some(session);
        m
    }

    /// A model with no session: it draws, it never plans.
    pub fn placeholder(settings: GuiSettings) -> AppModel {
        let (tx, rx) = channel();
        let preview = preview::PreviewModel::new(settings.live_encode);
        AppModel {
            picker: picker::Picker::default(),
            settings,
            settings_path: None,
            settings_base: None,
            settings_error: None,
            recovery_key: None,
            crash_restore: None,
            held_recovery: Default::default(),
            drawer_epoch: 0,
            folder_epoch: 0,
            user_config: None,
            user_presets: None,
            loaded_user_presets: vec![],
            skipped_user_presets: vec![],
            unreadable: vec![],
            recode: vec![],
            stale: vec![],
            stale_json: Default::default(),
            also_report: None,
            also_unreadable: None,
            read_only_flags: vec![],
            nif_problems: Default::default(),
            scan_db: None,
            scan_store_unsettled: false,
            unsettled_before_scan: false,
            scan_running: false,
            scan_stamp: None,
            scan_db_error: None,
            mesh_votes: Default::default(),
            mesh_alpha: Default::default(),
            mesh_tree_anim: Default::default(),
            evidence_rev: 0,
            gras_diffuses: Default::default(),
            gras_all_diffuses: Default::default(),
            known: Default::default(),
            used_by: Default::default(),
            original: Default::default(),
            json_editor_open: false,
            presets_open: false,
            centre_tab: CentreTab::Selection,
            presets_popped: false,
            json_editor_popped: false,
            reveal_centre: false,
            presets: None,
            accept: AcceptSuggestions::High,
            session: None,
            root: None,
            project: None,
            sets_doc: None,
            history: history::History::default(),
            target: Target::Cs,
            items: vec![],
            items_rev: 0,
            game_paths: (0, vec![]),
            dds_thumbs: Default::default(),
            dds_thumbs_asked: Default::default(),
            dds_thumbs_failed: Default::default(),
            dds_thumb_requests: 0,
            check_state: Default::default(),
            inputs_rev: 0,
            item_index: Default::default(),
            views: Default::default(),
            review_json_cache: Default::default(),
            rename_cache: Default::default(),
            sets: vec![],
            sets_root: None,
            unmatched_keys: vec![],
            rnm_notes: Default::default(),
            rnm_keys: Default::default(),
            batch_preset: String::new(),
            batch: false,
            selection: Selection::None,
            multi: Default::default(),
            bulk: Default::default(),
            filter: Filter::default(),
            prefs: Default::default(),
            texture_cache: Default::default(),
            search: String::new(),
            sets_filter: Default::default(),
            new_sets: None,
            sets_filter_cache: Default::default(),
            status_line: String::new(),
            messages: Default::default(),
            bottom: Default::default(),
            review: None,
            problem_builds: Default::default(),
            filter_builds: Default::default(),
            problem_lookups: Default::default(),
            export_files_builds: Default::default(),
            set_cards_drawn: Default::default(),
            sizes: Default::default(),
            rnm_preview: Default::default(),
            status_note: None,
            undo_note: None,
            undo_note_at: None,
            dropped_fills: Default::default(),
            dropped_fills_step: 0,
            encoding_note_step: 0,
            kept_banner: None,
            banner: None,
            banner_logged: vec![],
            encoding_note: None,
            dialog: None,
            marker: Default::default(),
            marker_note: None,
            last_recipe: Default::default(),
            expanded: None,
            export: export::ExportModel::default(),
            preview,
            dds_patch: Default::default(),
            view: view::ViewState::default(),
            overlay: Default::default(),
            uv: Default::default(),
            gpu_max_dim: 16384,
            gpu_available: true,
            export_after_plan: false,
            continue_after_plan: None,
            after_export: None,
            pending_json: None,
            pending_cs: None,
            pending_cm: None,
            pending_json_answer: None,
            nif_rows: vec![],
            nif_answer_request: None,
            nif_clear_answers_request: false,
            open_review_request: false,
            nif_diff_request: None,
            nif_export_queued: false,
            nif_export_due: false,
            also_pass: false,
            also_error: None,
            also_written: None,
            also_reply: None,
            also_json: Vec::new(),
            zip_limits: (engine::zip::MAX_ENTRIES, engine::zip::MAX_BYTES),
            also_cancel: None,
            also_tail: None,
            ignored_by_plan: false,
            replan_pushes: (0, None),
            close_ready: false,
            restart_needed: false,
            nvtt: None,
            nvtt_gen: 0,
            nvtt_pending: false,
            nvtt_key: None,
            plugin_refs: Default::default(),
            cs_pbr: Default::default(),
            nav: Default::default(),
            shell: Vec::new(),
            stats: Default::default(),
            gen_dims: Default::default(),
            gen_info: Default::default(),
            gen_look: Default::default(),
            gen: None,
            gen_select: None,
            after_commit: Vec::new(),
            weighting_follow: None,
            reencode_follow: None,
            gen_ids: 0,
            gen_opens: 0,
            report_saving: false,
            show_in_meshes: None,
            nif_tab_request: None,
            choose_preset: None,
            auto_note: None,
            missing_preset_id: None,
            last_inputs: None,
            auto_replans: 0,
            follow_off: false,
            type_pending: None,
            type_base: None,
            type_cause: false,
            type_cleaned: false,
            votes_gen: 0,
            votes_pending: None,
            generation: 0,
            planning: None,
            plan_progress: None,
            opening: None,
            open_cancelled: false,
            tx,
            rx,
        }
    }

    /// The header check and patch plan of DDS item `key` (DECISIONS S7); `None` for other items.
    pub fn dds_header(&mut self, key: &str) -> Option<dds_patch::DdsHeaderView> {
        let item = self.item(key).filter(|i| i.kind == ItemKind::Dds)?;
        let path = item.input.clone();
        Some(self.dds_patch.view(key, &path))
    }

    pub fn set_dds_patch_tag(&mut self, key: &str, srgb: Option<bool>) {
        self.dds_patch.set_tag(key, srgb);
    }

    /// Write the patched copy of DDS item `key` to `<export root>/<job key>` (Note FB1: only into a chosen folder). Every outcome lands in
    /// `dds_patch.last` (the panel shows it) and ends the confirmation.
    pub fn patch_dds_header(&mut self, key: &str) -> Result<PathBuf, String> {
        self.dds_patch.confirming = None;
        let r = match self.item(key).filter(|i| i.kind == ItemKind::Dds) {
            None => Err("not a DDS item".to_string()),
            Some(_) if self.export_folder_chosen().is_none() => {
                Err(review::NO_EXPORT_FOLDER.to_string())
            }
            Some(item) => {
                let (src, dest) = (item.input.clone(), self.export_root().join(&item.job_key));
                // Amended S7: never write over another item's output; the DDS's own copy may be re-patched.
                match self
                    .items
                    .iter()
                    .find(|o| o.key != key && o.job_key == item.job_key)
                {
                    Some(other) => Err(format!(
                        "{} already exports to {}; patch this DDS with `sdc patch-header --out-dir` into another folder",
                        other.key, item.job_key
                    )),
                    None => {
                        let tag = self.dds_header(key).and_then(|v| v.tag);
                        dds_patch::write_patch(&src, &dest, tag).map(|m| (m, dest))
                    }
                }
            }
        };
        let line = match &r {
            Ok((m, _)) => m.clone(),
            Err(e) => e.clone(),
        };
        // Note FB8: the panel shows the line without its code; the log keeps it whole.
        let severity = if r.is_ok() {
            messages::Severity::Info
        } else {
            messages::Severity::Warning
        };
        self.messages.log(severity, line.clone());
        self.dds_patch.last = Some((key.to_string(), line));
        r.map(|(_, dest)| dest)
    }

    pub fn open_folder(&mut self, root: PathBuf) {
        self.open_cancelled = false;
        // Whatever was pending for the previous folder no longer applies (spec §8).
        self.after_export = None;
        self.pending_json = None;
        self.pending_cs = None;
        self.pending_cm = None;
        self.pending_json_answer = None;
        self.nif_export_queued = false;
        self.nif_export_due = false;
        // A Generate of the previous folder: cancelled; one that writes still lands, and records nothing here (I1).
        self.gen_cancel();
        self.gen_select = None;
        self.weighting_follow = None;
        self.reencode_follow = None;
        self.after_commit.clear();
        self.drop_provenance_note();
        // Every dialog that can be open here belongs to the previous folder (`OpenUnsaved` and `ExportRunning`
        // were answered to get here).
        self.dialog = None;
        // Audit 2026-10-14 finding 2, Review Focus 2: Review and its ticks belong to the previous folder; a queued
        // Review export (cleared below) must not continue into an export of this one.
        self.close_review();
        // An export waiting for a plan of the previous folder no longer applies.
        self.export_after_plan = false;
        self.continue_after_plan = None;
        self.last_inputs = None;
        self.inputs_rev += 1;
        self.auto_replans = 0;
        self.reset_type_follow();
        self.drop_pending_votes();
        // Another folder's item keys can be the same strings: its list must not show here.
        self.used_by.current = None;
        self.used_by.wanted = None;
        self.used_by_scan_changed();
        // The source sizes read for the previous folder (AG129: the cache would grow for the whole session).
        self.sizes.clear_folder();
        // Audit item 22: the previous folder's file checks would stay for the session.
        self.stats.borrow_mut().clear();
        // Note FI1: the previous folder's DDS tile images.
        self.dds_thumbs.clear();
        self.dds_thumbs_asked.clear();
        self.dds_thumbs_failed.clear();
        self.drawer_epoch += 1;
        self.folder_epoch += 1;
        self.dropped_fills.clear();
        // Audit GM-6: the previous folder's header-patch result, tag choices and open confirmation.
        self.dds_patch = Default::default();
        self.scan_db_error = None;
        let same_folder = self.root.as_ref() == Some(&root);
        self.ignored_by_plan = false;
        let project = ProjectDoc::open(&root);
        self.project = Some(project);
        self.sets_doc = Some(crate::sets_doc::SetsDoc::open(&root));
        self.drop_undo_note();
        // Note FC12: the previous folder's history and selection are kept until this open completes (Cancel).
        let kept = (std::mem::take(&mut self.history), self.selection.clone());
        // `pushes` never goes back (FC3's rows compare it).
        self.history.pushes = kept.0.pushes;
        self.auto_note = None;
        self.refresh_banner();
        let recent = self.settings.recent.clone();
        self.settings.push_recent(root.clone());
        self.save_settings();
        let previous = self.root.replace(root);
        // A second open while the first is still planning goes back to the folder before both.
        if !(self.opening.is_some() && self.planning.is_some()) {
            self.opening = Some((previous, recent, kept.0, kept.1));
        }
        self.items.clear();
        self.check_state.plans_applied += 1;
        self.invalidate_views();
        self.sets.clear();
        self.sets_root = None;
        self.also_written = None;
        self.also_report = None;
        self.also_unreadable = None;
        // FC11: the previous folder's stale files never get a Delete here.
        self.stale.clear();
        self.stale_json.clear();
        self.unmatched_keys.clear();
        self.selection = Selection::None;
        self.reset_multi();
        self.bulk = Default::default();
        self.expanded = None;
        self.preview_close();
        // The drawer shows the folder it ran on; another folder starts with an empty one.
        if !same_folder && self.export.running.is_none() {
            self.export = export::ExportModel::default();
        }
        self.apply_recovery();
        self.refresh_banner(); // a read-only document's held recovery file
        self.replan();
    }

    pub fn save_settings(&mut self) {
        let Some(p) = self.settings_path.clone() else {
            return;
        };
        match self.settings.save_merged(&p, &mut self.settings_base) {
            Ok(kept) => {
                self.settings_error = None;
                if let Some(kept) = kept {
                    self.messages.notify(
                        messages::Severity::Warning,
                        format!(
                            "gui.toml: it could not be read, so it was kept as {} and a new one was written",
                            kept.display()
                        ),
                    );
                }
            }
            Err(e) => {
                let text = format!("gui.toml: could not save: {e}");
                if self.settings_error.as_ref() != Some(&text) {
                    self.messages
                        .notify(messages::Severity::Warning, text.clone());
                    self.settings_error = Some(text);
                }
            }
        }
    }

    /// The export folder the current sets were planned with (their keys are under it), else `export_root`.
    pub fn sets_root(&self) -> PathBuf {
        self.sets_root.clone().unwrap_or_else(|| self.export_root())
    }

    /// The export folder the user chose for this project (Note FB1): with a usable project document (loaded, not
    /// read-only) its `[project] export_folder`, and nothing else, so a folder picked for another mod is never
    /// inherited. Without one (no folder open, or an `sdc-project.toml` that does not parse) the pick lives in
    /// `gui.toml` (Note BV6), so that is the choice.
    pub fn export_folder_chosen(&self) -> Option<PathBuf> {
        match self.sets_doc.as_ref().filter(|d| !d.read_only()) {
            Some(d) => d.export_folder(),
            None => self.settings.export_folder.clone(),
        }
    }

    /// Where an export would write: the chosen folder, else `<root>/sdc-out` (only a place to name; Review blocks the
    /// export until a folder is chosen, Note FB1).
    pub fn export_root(&self) -> PathBuf {
        self.export_folder_chosen()
            .unwrap_or_else(|| self.root.clone().unwrap_or_default().join("sdc-out"))
    }

    /// What the next plan uses: the folder, recursive, High suggestions accepted (Note U3), the in-memory `sdc.toml` (DECISIONS N4).
    pub fn request(&self) -> Option<BatchRequest> {
        let root = self.root.clone()?;
        let project = self.project.as_ref()?;
        let mut req = BatchRequest::new(vec![root], self.output_root(), Target::Cs);
        // TA10, P15: while a tick is on the plan is the PBR phase's (its folder, the whole location skipped).
        if self.also_root().is_some() {
            req.skip_root = Some(self.also_location());
        }
        req.target = None;
        req.recursive = true;
        req.accept = self.accept;
        req.project_config_inline = Some(if project.read_only() {
            ConfigFile::default()
        } else {
            let mut c = project.config().unwrap_or_default();
            // AM14: a preset that no longer exists is left out, so the plan runs on the fallback; Note FC1: the
            // export is blocked by a row until another preset is chosen.
            if self.missing_preset_id.is_some() {
                c.preset = None;
            }
            c
        });
        req.user_config = self.user_config.clone();
        req.user_presets = self.user_presets.clone();
        req.jobs = self.settings.jobs;
        req.mem_budget = self.settings.mem_budget_mb * 1024 * 1024;
        req.nvtt = self.settings.nvtt_path.clone();
        let (edits, paths) = sets::engine_inputs(self);
        req.set_edits = edits;
        req.output_paths = paths;
        req.shared_roughness = sets::shared_roughness(self);
        req.borrowed_normal = sets::borrowed_normal(self);
        req.set_shaders = sets::set_shaders(self);
        req.pinned_files = sets::pinned_files(self);
        req.mesh_shaders = self.mesh_votes.clone();
        req.mesh_alpha = self.mesh_alpha.clone();
        req.gras_diffuses = self.gras_diffuses.clone();
        req.gras_all_diffuses = self.gras_all_diffuses.clone();
        req.cs_txst_sets = sets::cs_txst_sets(self);
        Some(req)
    }

    /// S-shader-10, M6 Track E: read the mesh votes, the mesh alpha (keys from the colour-member jobs only, by
    /// `shader::colour_mesh_key`, the engine's rule) and the GRAS diffuses (complex grass design §5 condition 1) from
    /// the scan database on one worker thread, tagged with a generation; `votes_landed` applies the newest result.
    /// No database means no evidence (applied at once): the suffix rules decide and the alpha is asked. A read
    /// error keeps the previous values (a Messages warning). `after_plan`: the result continues `apply_plan`'s tail.
    pub fn refresh_mesh_votes(&mut self, after_plan: bool) {
        let keys = self.mesh_vote_keys();
        // A scan-finished refresh must not drop the continuation of a pending post-plan read.
        if !after_plan {
            // A finished scan (or another database): the "Used by meshes" list reads again.
            self.used_by_scan_changed();
            // And UV coverage, a session cache read from the scan.
            self.uv.clear();
        }
        let after_plan = after_plan || self.votes_pending.is_some_and(|(_, a)| a);
        self.votes_gen += 1;
        let generation = self.votes_gen;
        self.votes_pending = Some((generation, after_plan));
        // Wave 2 #6: the store reads empty while a scan runs or after one was cancelled; keep what we have.
        if self.scan_store_unsettled {
            let kept = MeshEvidence {
                votes: self.mesh_votes.clone(),
                alpha: self.mesh_alpha.clone(),
                tree_anim: self.mesh_tree_anim.clone(),
                gras: self.gras_diffuses.clone(),
                gras_all: self.gras_all_diffuses.clone(),
            };
            return self.votes_landed(generation, Ok(kept));
        }
        let Some(db) = self.scan_db.clone().filter(|d| d.is_file()) else {
            return self.votes_landed(generation, Ok(Default::default()));
        };
        spawn_guarded(self.tx.clone(), "mesh-votes", move |tx| {
            let read = || -> Result<MeshEvidence, String> {
                Ok(MeshEvidence {
                    votes: meshes::mesh_votes(&db, &keys)?,
                    alpha: meshes::mesh_alpha(&db, &keys)?,
                    tree_anim: meshes::mesh_tree_anim(&db, &keys).unwrap_or_default(),
                    gras: meshes::gras_diffuses(&db)?,
                    gras_all: meshes::gras_all_diffuses(&db)?,
                })
            };
            // A panic is an error of this generation (kept values, a warning), not a dialog for a stale read.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read))
                .unwrap_or_else(|p| Err(format!("panic: {}", crate::worker::panic_text(&*p))));
            let _ = tx.send(WorkerMsg::MeshVotes { generation, result });
        });
    }

    /// A Meshes scan started: `begin_scan` disables every origin, so the store reads empty until it ends.
    pub fn scan_started(&mut self) {
        self.unsettled_before_scan = self.scan_store_unsettled;
        self.scan_store_unsettled = true;
        self.scan_running = true;
        // A read in flight may see the disabled origins: the same read again lands the kept evidence at once and
        // makes the worker's answer stale (it keeps its `after_plan` tail).
        if let Some((_, after_plan)) = self.votes_pending {
            self.refresh_mesh_votes(after_plan);
        }
    }

    /// The scan ended. `completed`: it finished; `store_untouched`: it stopped before `begin_scan` because PGPatcher
    /// is running, so the store still holds what it held before (not Busy: another scan is mid-flight; Note CW2).
    pub fn scan_finished(&mut self, completed: bool, store_untouched: bool) {
        self.scan_running = false;
        self.scan_store_unsettled = if store_untouched {
            self.unsettled_before_scan
        } else {
            !completed
        };
        if completed {
            self.refresh_mesh_votes(false);
            self.refresh_known_paths();
        }
    }

    /// The mesh keys `refresh_mesh_votes` reads: each colour member's (by `shader::colour_mesh_key`); while a tick
    /// is on also its vanilla key (`derive::vanilla_game_path`, TA13), the one the env-map gate looks up.
    pub fn mesh_vote_keys(&self) -> std::collections::BTreeSet<String> {
        let also = self.also_on();
        self.items
            .iter()
            .filter_map(|i| i.job.as_ref())
            .filter(|j| j.pack.is_none())
            .flat_map(|j| {
                let key = |g: &str| {
                    engine::shader::colour_mesh_key(g, &j.output, Some(j.profile.subtype))
                };
                let vanilla = also
                    .then(|| key(&engine::derive::vanilla_game_path(&j.game_path)))
                    .flatten();
                key(&j.game_path).into_iter().chain(vanilla)
            })
            .collect()
    }

    /// A newer plan or another folder makes the pending scan-database read stale: its result is dropped.
    fn drop_pending_votes(&mut self) {
        self.votes_gen += 1;
        self.votes_pending = None;
    }

    /// The scan-database read of generation `generation` ended; a stale one is dropped.
    fn votes_landed(&mut self, generation: u64, result: Result<MeshEvidence, String>) {
        if generation != self.votes_gen {
            return;
        }
        let Some((_, after_plan)) = self.votes_pending.take() else {
            return;
        };
        let changed = match result {
            Ok(e) => {
                self.scan_db_error = None;
                let changed = e.votes != self.mesh_votes
                    || e.alpha != self.mesh_alpha
                    || e.gras != self.gras_diffuses
                    || e.gras_all != self.gras_all_diffuses;
                (self.mesh_votes, self.mesh_alpha, self.gras_diffuses) = (e.votes, e.alpha, e.gras);
                self.gras_all_diffuses = e.gras_all;
                self.set_tree_anim(e.tree_anim);
                changed
            }
            Err(e) => {
                self.scan_db_warning(format!(
                    "Scan database: {e}; keeping the previous mesh evidence"
                ));
                false
            }
        };
        if after_plan {
            // A newer plan in flight finishes (and reads again) itself.
            if self.planning.is_none() {
                self.finish_plan();
            }
        } else if changed {
            // The plan this starts carries a queued export to its end (`finish_plan`).
            self.replan();
        } else {
            // DECISIONS AF4: an Export (the header's or Review's) clicked during this scan-finished read starts now,
            // never on a later plan. Both are taken together, as in `finish_plan`; the export the user clicked wins.
            let (clicked, continued) = (
                std::mem::take(&mut self.export_after_plan),
                self.continue_after_plan.take(),
            );
            match continued {
                _ if (clicked || continued.is_some()) && self.dialog.is_some() => {
                    self.queued_export_dropped()
                }
                _ if clicked => self.export_clicked(),
                // A plan may still be in flight, or sources changed: the same front half as Review's Export.
                Some(_) => self.continue_review(),
                None => {}
            }
        }
    }

    /// Plan again on a worker; a newer plan supersedes (cancels) an older one.
    pub fn replan(&mut self) {
        // Note FC1: read the presets once per plan, not per frame (the export gate asks every frame).
        self.missing_preset_id = self.missing_preset();
        self.refresh_banner();
        let (Some(session), Some(req), Some(root)) =
            (self.session.clone(), self.request(), self.root.clone())
        else {
            return;
        };
        if let Some(c) = self.planning.take() {
            c.cancel();
        }
        self.generation += 1;
        // A re-plan with no step pushed since (a scan, a superseded plan) keeps the blame of the plan it replaces.
        let (last, blame) = self.replan_pushes;
        let pushes = self.history.pushes;
        self.replan_pushes = (pushes, if pushes != last { Some(pushes) } else { blame });
        self.drop_pending_votes();
        let cancel = CancelToken::default();
        self.planning = Some(cancel.clone());
        self.plan_progress = None;
        self.status_line = "Planning…".into();
        self.inputs_rev += 1;
        self.last_inputs = Some(EngineInputs {
            set_edits: req.set_edits.clone(),
            output_paths: req.output_paths.clone(),
            shared_roughness: req.shared_roughness.clone(),
            borrowed_normal: req.borrowed_normal.clone(),
            set_shaders: req.set_shaders.clone(),
            pinned_files: req.pinned_files.clone(),
            mesh_votes: req.mesh_shaders.clone(),
            mesh_alpha: req.mesh_alpha.clone(),
            gras_diffuses: req.gras_diffuses.clone(),
            gras_all_diffuses: req.gras_all_diffuses.clone(),
            cs_txst_sets: req.cs_txst_sets.clone(),
        });
        spawn_plan(self.tx.clone(), session, req, cancel, self.generation, root);
    }

    /// Item 13: the header offers Cancel while a folder's first plan is in flight (not for a re-plan, AG93).
    pub fn can_cancel_open(&self) -> bool {
        self.opening.is_some() && self.planning.is_some()
    }

    /// Cancel the open in flight (AG92): its plan is cancelled and, by the generation bump, dropped when it reports.
    /// The previous folder opens again (its analyses are still cached), else the window is empty with a note.
    pub fn cancel_open(&mut self) {
        if !self.can_cancel_open() {
            return;
        }
        if let Some(c) = self.planning.take() {
            c.cancel();
        }
        self.generation += 1;
        self.plan_progress = None;
        let Some((previous, recent, mut history, selection)) = self.opening.take() else {
            return;
        };
        // AG93: Recent is as it was before the open, so a cancelled folder new to it is not listed.
        self.settings.recent = recent;
        self.save_settings();
        match previous {
            Some(previous) => {
                self.open_folder(previous);
                // Going back is not an open the user can cancel.
                self.opening = None;
                // Note FC12 (supersedes AG92's reset): the folder comes back with its undo history and selection. Its
                // documents were clean when the open started (saved, or their edits dropped and the history cleared),
                // so the re-read files are where the history left them. Note FC14: unless the re-open restored crash
                // edits as a step; then its history is the one that matches the documents (and the toast's Undo).
                if self.history.undo.is_empty() {
                    history.pushes = history.pushes.max(self.history.pushes);
                    self.history = history;
                }
                self.selection = selection;
            }
            None => {
                self.root = None;
                self.project = None;
                self.sets_doc = None;
                self.last_inputs = None;
                // An export queued behind the cancelled plan has no folder to run on (AG129).
                self.export_after_plan = false;
                self.continue_after_plan = None;
                self.close_review();
                self.refresh_banner();
                self.status_line.clear();
                self.open_cancelled = true;
            }
        }
    }

    /// The hover of the header's Cancel while a folder opens.
    pub fn cancel_open_hover(&self) -> &'static str {
        match &self.opening {
            Some((Some(_), ..)) => {
                "Stops opening and goes back to the previous folder, with its selection and undo history"
            }
            _ => "Stops opening; no folder stays open",
        }
    }

    /// The centre's text with no folder open.
    pub fn empty_hint(&self) -> &'static str {
        if self.open_cancelled {
            "Opening cancelled"
        } else {
            "Open a folder of source textures to begin."
        }
    }

    /// A sender into the model's own channel (tests inject worker messages with it).
    pub fn sender(&self) -> Sender<WorkerMsg> {
        self.tx.clone()
    }

    pub fn busy(&self) -> bool {
        self.planning.is_some()
            || self.votes_pending.is_some()
            || self.export.running.is_some()
            || self.preview.working()
            || self.nvtt_pending
    }

    /// A v2 worker job is in flight (owner answer 8): the app's one poll line keeps frames coming and `wait_idle`
    /// waits for it. Not part of `busy()`, which greys the Advanced fields. Tracks A and B add their jobs here.
    pub fn v2_jobs_pending(&self) -> bool {
        self.overlay_pending()
            || self.uv.read_pending()
            || self.uv.all_running()
            || self.uv.probe_pending()
            || self.gen_pending()
    }

    /// Drain worker messages. True when anything changed (the UI repaints).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        // Note FC13 (CD16): a changed filter ends the "new set stays shown" record for good.
        if self
            .new_sets
            .as_ref()
            .is_some_and(|(f, _)| *f != self.sets_filter)
        {
            self.new_sets = None;
        }
        // Note FC5 (GD2): the NIF footer's `Review…` only sets the flag; the next poll opens Review.
        if std::mem::take(&mut self.open_review_request) {
            self.open_review();
            changed = true;
        }
        self.scan_stamp = cs_pbr::stamp(self.scan_db.as_deref());
        self.uv_poll();
        self.cs_pbr.new_frame();
        for d in self.picker.take() {
            changed = true;
            self.picked(d);
        }
        let mut thumbs_landed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            match msg {
                WorkerMsg::DdsThumb { path, stamp, thumb } => {
                    thumbs_landed |= self.dds_thumb_landed(path, stamp, thumb)
                }
                msg => self.handle(msg),
            }
        }
        // Note FI1: one invalidation for every DDS tile image of this poll (the tile cache keys on the revision).
        if thumbs_landed {
            self.invalidate_views();
        }
        // Last, so a plan that landed above is checked in this poll (K1).
        changed |= self.refresh_checks();
        changed
    }

    fn handle(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::PlanProgress {
                generation,
                done,
                total,
            } if generation == self.generation => {
                self.plan_progress = Some((done, total));
                self.status_line = plan_bar_text(done, total);
            }
            WorkerMsg::PlanProgress { .. } => {}
            WorkerMsg::Planned { generation, result } if generation == self.generation => {
                self.planning = None;
                self.plan_progress = None;
                self.opening = None;
                match result {
                    Ok(view) => {
                        // Note FC14: the edit planned fine; a later failure is not its fault.
                        self.replan_pushes.1 = None;
                        self.apply_plan(view)
                    }
                    Err(f) => self.plan_failed(f),
                }
            }
            WorkerMsg::Planned { .. } => {}
            WorkerMsg::MeshVotes { generation, result } => self.votes_landed(generation, result),
            WorkerMsg::UsedBy { generation, result } => self.used_by_landed(generation, result),
            WorkerMsg::OriginalTextures {
                generation,
                key,
                result,
            } => self.original_landed(generation, key, result),
            WorkerMsg::KnownPaths(result) => self.known_paths_landed(result),
            WorkerMsg::Nvtt { generation, probe } => self.nvtt_landed(generation, probe),
            WorkerMsg::Panicked { what, message } => {
                match what {
                    // Audit 08f-9: one error dialog. `export_ended` shows it unless the user was leaving (then the
                    // panic arm shows its own, Note BI6) or a dialog is open, which stays (review m1, 08f-6).
                    "export" => {
                        // Final review I2: the worker is gone; nothing waits for the JSON and no `AlsoZipped` comes.
                        self.also_reply = None;
                        self.also_tail = None;
                        self.export_ended(Err(format!("internal error: {message}")));
                        if self.dialog.is_some() {
                            return;
                        }
                    }
                    "dims" => self.dims_panicked(),
                    "used-by" => self.used_by_panicked(&message),
                    "original-textures" => self.original_panicked(&message),
                    "save-report" => self.report_saving = false,
                    "dds thumbnails" => self.dds_thumbs_asked.clear(),
                    _ => {}
                }
                self.raise_error(format!("internal error in {what}: {message}"));
            }
            other => self.handle_more(other),
        }
    }

    /// A plan failed. When the engine rejected the project config that parsed fine (an unknown field or code, an
    /// out-of-range value), ignore `sdc.toml` as for a parse error (spec §7.4): read-only, banner, and plan the
    /// folder again without it. Any other failure (or a config error with `sdc.toml` already ignored) is a dialog.
    fn plan_failed(&mut self, f: PlanFailure) {
        self.export_after_plan = false;
        self.continue_after_plan = None;
        // An error naming the user config is not the project's fault; ignoring `sdc.toml` would not help.
        let from_user_config = self
            .user_config
            .as_ref()
            .is_some_and(|p| f.message.contains(&p.display().to_string()));
        if f.config && !from_user_config {
            if let Some(p) = self.project.as_mut().filter(|p| !p.read_only()) {
                p.set_error(&f.message);
                self.drop_undo_note();
                // Note FC12 (supersedes BN1's clear): the history stays. The edit that failed is the top step, named
                // with the error, and Undo takes it back (`apply_snapshot` lifts the blame first). Note FC14: only
                // while that step's edit started this plan (nothing pushed since), never an older or later step.
                let edit_on_top = self.replan_pushes.1 == Some(self.history.pushes);
                if let Some(top) = self
                    .history
                    .undo
                    .last_mut()
                    .filter(|c| edit_on_top && !c.label.contains(REJECTED))
                {
                    top.label = format!("{}{REJECTED}{})", top.label, f.message);
                }
                self.refresh_banner();
                self.ignored_by_plan = true;
                self.replan();
                return;
            }
            // The plan without `sdc.toml` failed too: the config is not the project's (the user config, say).
            // Put the project back as it was, unsaved edits included (trace-debug #9), and raise the error.
            if std::mem::take(&mut self.ignored_by_plan) {
                if let Some(p) = self.project.as_mut() {
                    p.unblame();
                    self.drop_undo_note();
                    // Note FC12: the history was kept; the top step was not the culprit after all.
                    if let Some(top) = self.history.undo.last_mut() {
                        if let Some(i) = top.label.find(REJECTED) {
                            top.label.truncate(i);
                        }
                    }
                    self.refresh_banner();
                }
            }
        }
        self.status_line = format!("Planning failed: {}", f.message);
        self.raise_error(f.message);
    }

    /// An error dialog. (#48's "unless the overwrite question is open" ended with that question, Note FC5.)
    fn raise_error(&mut self, msg: String) {
        self.dialog = Some(Dialog::Error(msg));
    }

    /// Messages the export and the preview handle.
    fn handle_more(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::ReportSaved { path, result } => self.report_saved(&path, result),
            WorkerMsg::Event(e) => self.export.on_event(e),
            WorkerMsg::AlsoFinished(s, reply) => {
                self.also_reply = Some(reply);
                self.also_finished(*s)
            }
            WorkerMsg::AlsoZipped(s) => self.also_zipped(*s),
            WorkerMsg::ExportDone(result) => self.export_ended(result),
            WorkerMsg::Source { token, result } => self.on_source(token, result),
            WorkerMsg::Encoded { token, result } => self.on_encoded(token, result),
            WorkerMsg::Heatmap { id, image } => self.heatmap_landed(id, image),
            WorkerMsg::Overlay { generation, image } => self.overlay_landed(generation, image),
            WorkerMsg::UvCoverage {
                generation,
                tex,
                size,
                coverage,
            } => self.uv_landed(generation, (tex, size), coverage),
            WorkerMsg::UvProgress {
                run,
                done,
                total,
                finished,
            } => self.uv_progress(run, done, total, finished),
            WorkerMsg::ScanHasRows { stamp, rows } => self.uv_rows_landed(stamp, rows),
            WorkerMsg::Dims(dims) => self.dims_landed(dims),
            WorkerMsg::RnmPreview { id, image } => self.rnm_preview_landed(id, image),
            WorkerMsg::Generated {
                id,
                preview,
                result,
            } => self.gen_landed(id, preview, result),
            _ => {}
        }
    }

    /// The export ended: its result, or a worker panic as an error (audit GM-4: one path for both). The pace clock
    /// stops, the JSON/CS/CM files are written or noted, and an error is always logged.
    fn export_ended(&mut self, result: Result<engine::api::BatchSummary, String>) {
        let token = self.export.running.take();
        let cancelled = token.as_ref().is_some_and(|c| c.is_cancelled());
        // Note FC5: a cancelled export writes no NIF edits either (as `finish_json` writes no JSON).
        if cancelled || result.as_ref().is_ok_and(|s| !s.cancelled.is_empty()) {
            self.abandon_nif_export();
        }
        if self.also_reply.is_some() {
            self.also_cancel = token;
        }
        if self.also_pass {
            // Note CN: every row ends, whatever ended the export (a refusal or plan error leaves planned rows with no
            // `BatchDone`; a cancelled copy has no event).
            let why = match &result {
                Err(e) => Some(e.clone()),
                Ok(s) => (!cancelled && s.cancelled.is_empty())
                    .then(|| self.also_error.clone())
                    .flatten(),
            };
            let cancelled = cancelled || result.as_ref().is_ok_and(|s| !s.cancelled.is_empty());
            self.export.end_unfinished(cancelled, why.as_deref());
        }
        if let Some(p) = self.export.pace.as_mut() {
            p.stop(Instant::now());
        }
        let mut failure = None;
        let mut completed = false;
        match result {
            Ok(summary) => {
                let sev = if self.also_error.is_some() {
                    // Note CN: a Vanilla or CM phase failed; the PBR folder is complete, its JSON is still written.
                    messages::Severity::Error
                } else if summary.failed > 0 {
                    messages::Severity::Warning
                } else {
                    messages::Severity::Info
                };
                self.messages.notify(sev, export_finished_text(&summary));
                completed = summary.cancelled.is_empty();
                self.export.summary = Some(summary);
            }
            Err(e) => {
                self.messages
                    .log(messages::Severity::Error, format!("Export failed: {e}"));
                self.export.error = Some(e.clone());
                failure = Some(e);
            }
        }
        self.finish_export(completed, failure);
    }

    /// After the export: the JSON/CS/CM files, the checks summary once, then what was asked while the export ran
    /// (close, or open another folder), else its error as a dialog.
    fn finish_export(&mut self, completed: bool, failure: Option<String>) {
        // CL19: only a completed export gets the checks line; a cancel logs just "Export finished".
        if let Some(t) = checks::checks_summary(self).filter(|_| completed) {
            self.messages.notify(messages::Severity::Info, t);
        }
        self.finish_json();
        let cs = self.pending_cs.take();
        self.finish_cs_pbr(cs);
        let cm = self.pending_cm.take();
        self.finish_cm_json(cm);
        // FC11: the records just written decide which JSON files are stale.
        self.refresh_stale_json();
        // Final review I2: an Also export's walk, zips and report run now, on its worker, so the PBR zip holds the
        // JSON above. The export stays running (Cancel reaches the zips); `also_zipped` does the rest.
        let json = std::mem::take(&mut self.also_json);
        if let Some(tx) = self.also_reply.take() {
            if tx.send(json).is_ok() {
                self.export.running = Some(self.also_cancel.take().unwrap_or_default());
                self.also_tail = Some(failure);
                return;
            }
        }
        self.after_export(failure);
    }

    /// Note GF9: a cancelled run drops the NIF export it queued and the Review answers given for it (GD29), so the
    /// next Review asks again; `app.rs` calls `clear_nif_answers`.
    pub(super) fn abandon_nif_export(&mut self) {
        if std::mem::take(&mut self.nif_export_queued) {
            self.nif_clear_answers_request = true;
        }
    }

    /// The end of an export: what was asked while it ran (close, or open another folder), else its error as a dialog.
    fn after_export(&mut self, failure: Option<String>) {
        // Note FC5: the run has fully ended (with Also on, after its zips). Opening another folder below clears it.
        self.nif_export_due |= std::mem::take(&mut self.nif_export_queued);
        // A user who cancelled the export to leave does not need its error as a dialog.
        if !self.run_after_export() {
            // Audit 08f-6: an open dialog stays; the error is in the Files tab and the Messages log.
            if let Some(e) = failure.filter(|_| self.dialog.is_none()) {
                self.dialog = Some(Dialog::Error(e));
            }
        }
    }

    /// Whether `sdc.toml` can be saved: `Err(reason)` with no project, a load error, or the read-only flag. A missing
    /// file is fine: the save creates it. Stats the file: call it once. The reason is the hover of Review's disabled
    /// Half (Note FC2); the Also rows show their own text.
    pub fn sdc_toml_writable(&self) -> Result<(), String> {
        let Some(p) = self.project.as_ref() else {
            return Err("No project is open.".into());
        };
        if p.read_only() {
            return Err("sdc.toml could not be loaded, so it is read-only here.".into());
        }
        match std::fs::metadata(p.path()) {
            Ok(m) if m.permissions().readonly() => Err(format!(
                "{} is marked read-only, so the half-size choice cannot be saved.",
                p.path().display()
            )),
            _ => Ok(()),
        }
    }

    /// I1: a close or open asked while a Generate writes: done when it has landed and recorded its files, so a file
    /// Tessalith wrote is never left unrecorded or recorded into another project.
    fn defer_until_generated(&mut self, action: AfterExport) {
        self.status_line = match action {
            AfterExport::Close => "Closing when Generate has finished",
            AfterExport::Open(_) => "Opening the folder when Generate has finished",
        }
        .into();
        self.after_export = Some(action);
    }

    /// Tests: end the export with a made-up result (`WorkerMsg` is private), as the worker's `ExportDone` would.
    #[doc(hidden)]
    pub fn end_export_with(&mut self, result: Result<engine::api::BatchSummary, String>) {
        self.export_ended(result);
    }

    /// An export waits for a plan to land (Export clicked while planning, or a Continue after a re-plan).
    pub fn export_queued(&self) -> bool {
        self.export_after_plan || self.continue_after_plan.is_some()
    }

    /// Export the folder; first the checks that need an answer (M3a spec §2.5). Sources that changed since the plan
    /// are flagged and the plan is made again first, so the export starts from fresh plans (M3a spec §7).
    pub fn export_clicked(&mut self) {
        self.status_note = None;
        if self.root.is_none() {
            return;
        }
        // A plan is in flight (just opened, or an edit): the items on screen are the previous plan's, so the checks
        // wait for the plan that lands (spec §2.6: the button is never silently disabled).
        if self.planning.is_some() || self.votes_pending.is_some() {
            self.export_after_plan = true;
            return;
        }
        if self.check_sources() > 0 && self.session.is_some() {
            self.export_after_plan = true;
            self.replan();
            return;
        }
        self.export_checks();
    }

    /// The guards that stop an export outright, in order: the plan on screen is not the one for these sets, the
    /// export folder is unusable, an `export_blocks` entry. True (with the error dialog, or Review listing the
    /// blocks) when one fired. Shared by `export_checks` and `Continue::Review`, whose decisions can change them.
    fn export_stopped(&mut self) -> bool {
        let Some(root) = self.root.clone() else {
            return true;
        };
        if self.plan_unsettled() {
            return true;
        }
        // P14: with a tick on, Review's gate checks the location and the three folders instead (TA19).
        if let Some(problem) = (!self.also_on())
            .then(|| export::export_folder_problem(&root, &self.export_root()))
            .flatten()
        {
            self.dialog = Some(Dialog::Error(problem));
            return true;
        }
        let blocks = pbr_export::export_blocks(self);
        if !blocks.is_empty() {
            self.open_review();
            return true;
        }
        false
    }

    /// The export checks, then the export itself. C2: a clean project exports without Review; blocks, `export_asks`
    /// pitfalls or undecided textures open Review instead (R1).
    fn export_checks(&mut self) {
        // Error dialog, or blocks: Review.
        if self.export_stopped() {
            return;
        }
        let asks = !pbr_export::export_asks(self).is_empty();
        // R6: an edited PGPatcher JSON is asked about in Review, before the export (C4).
        // Note CN (C1, owner answer 5): a tick opens Review; its Export is the only way to an Also export.
        // Note FB1: no chosen folder is a Review blocker, so the click lists it there instead of writing to `sdc-out`.
        // Note FC2: the half-size question is asked in Review, before the export.
        // Note FC5: NIF edits are Review rows; its Export is the only way they are written.
        if asks
            || self.nif_rows_written()
            || !self.half_size_rows().is_empty()
            || self.undecided() > 0
            || self.pgp_json_edited()
            || self.also_on()
            || self.export_folder_chosen().is_none()
        {
            self.open_review();
            return;
        }
        self.start_export_pass();
    }

    /// True (with an error dialog) when the plan on screen is not the one for these sets.
    fn plan_unsettled(&mut self) -> bool {
        if !self
            .last_inputs
            .as_ref()
            .is_some_and(|l| *l != EngineInputs::of(self))
        {
            return false;
        }
        let mut msg = String::from(
            "The sets changed while planning (two outputs may share one path), so nothing was exported. \
             Check set names and the match/rename fields, then export again.",
        );
        for c in pbr_export::path_collisions(self) {
            msg.push('\n');
            msg.push_str(&c.text);
        }
        self.dialog = Some(Dialog::Error(msg));
        true
    }

    /// Keys of the undecided textures, in list order.
    pub fn undecided_keys(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::NeedsDecision)
            .map(|i| i.key.clone())
            .collect()
    }

    /// Whether the texture asks for its sub-type, and the engine's suggestion for it (the one the confirm-subtype
    /// box preselects). `None`: the sub-type is not what it waits for (an alpha question).
    pub fn subtype_question(&self, key: &str) -> Option<Option<Subtype>> {
        self.item(key)?.decisions.iter().find_map(|d| match d {
            Decision::Subtype { suggested } => Some(*suggested),
            _ => None,
        })
    }

    /// One undecided row in Review: decide `key` as `st`. The row leaves the list when the plan that follows shows
    /// the texture decided (it may ask something else next). R2: deciding the last row never exports.
    pub fn decide_undecided_row(&mut self, key: &str, st: Subtype) {
        self.decide(key, Answer::Subtype(st));
    }

    /// A row's inline answer that is not a sub-type (alpha meaning, an acknowledgement). The row
    /// leaves the list when the plan that follows shows the texture decided.
    pub fn answer_undecided_row(&mut self, key: &str, answer: Answer) {
        self.decide(key, answer);
    }

    /// "Accept all suggestions": decide every listed texture that has a suggestion as that suggestion, as one undo
    /// step. A texture without a suggestion stays listed and undecided; alpha and acknowledgements are
    /// never answered here.
    pub fn accept_all_suggestions(&mut self) {
        let keys = self.undecided_keys();
        let mut picks: Vec<(String, Subtype)> = keys
            .into_iter()
            .filter_map(|k| {
                let st = self.subtype_question(&k)??;
                Some((k, st))
            })
            .collect();
        if picks.is_empty() {
            return;
        }
        // PBR colour picks lead: deciding a set's diffuse as `pbr_albedo` or `skin_albedo` switches its inferred
        // members (PBR follow), so those members must be looked at afterwards, not decided from the plan's older
        // suggestions.
        picks.sort_by_key(|(_, st)| !(st.is_pbr_color() || sets::is_pbr_diffuse(*st)));
        // Audit 11 (Note Y8): one snapshot, every pick, one step, one plan (not one plan thread per pick).
        self.batch_edit("Accept all suggestions", |m| {
            for (k, st) in &picks {
                let written = m
                    .project
                    .as_ref()
                    .is_some_and(|p| p.file_value(k, "subtype").is_some());
                if !written {
                    m.decide(k, Answer::Subtype(*st));
                }
            }
        });
    }

    /// A user edit recorded a step: re-plan as a change (`replan_changed`).
    fn replan_after_edit(&mut self) {
        self.type_edit_recorded();
        self.replan_changed();
    }

    /// `replan` for a change the user made to the project (an edit, an undo or a redo): an Export queued behind a
    /// scan-finished read is cancelled with a note (DECISIONS AF4, AF10, AF11).
    fn replan_changed(&mut self) {
        let cancel = self.export_after_plan
            && self.planning.is_none()
            && matches!(self.votes_pending, Some((_, false)));
        self.replan();
        if cancel {
            self.export_after_plan = false;
            self.status_note = Some(
                "Export cancelled: the project changed while scan results were loading — click Export again."
                    .into(),
            );
            self.status_line = self.status_note.clone().unwrap_or_default();
            self.messages
                .log(messages::Severity::Warning, self.status_line.clone());
        }
    }

    /// Run `f`'s edits as one undo step and one plan (audit 11, F3a D5, Note Z3): one snapshot before, `edit_docs`
    /// writes without steps or plans meanwhile, then one step and one `replan` when the text changed. True when it did.
    fn batch_edit(&mut self, label: &str, f: impl FnOnce(&mut AppModel)) -> bool {
        // Note DU6: a batch inside a batch (a sub-type edit with its recipe follow) joins the outer step.
        if self.batch {
            f(self);
            return true;
        }
        let before = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        let dirty = (
            self.project.as_ref().is_some_and(ProjectDoc::is_dirty),
            self.sets_doc
                .as_ref()
                .is_some_and(crate::sets_doc::SetsDoc::is_dirty),
        );
        self.batch = true;
        f(self);
        self.batch = false;
        let after = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        if after == before {
            // Trace #28: writes that changed nothing leave the documents as clean as they were.
            if let Some(p) = self.project.as_mut() {
                p.set_dirty(dirty.0);
            }
            if let Some(s) = self.sets_doc.as_mut() {
                s.set_dirty(dirty.1);
            }
            return false;
        }
        self.drop_undo_note();
        self.history.push(history::Command {
            label: label.into(),
            before,
            after,
            merge: None,
            created_file: None,
        });
        self.auto_replans = 0;
        self.replan_after_edit();
        true
    }

    fn run_continue(&mut self, how: Continue) {
        match how {
            Continue::Review => {
                // K4: Review closed (another folder, Reload) drops its queued export.
                if self.review.is_none() {
                    return;
                }
                // Note GH8: no project means no texture run (`start_export_pass` needs one): the NIF export is due now,
                // and Review closes as `start_export_pass` closes it, keeping its NIF answers for that export.
                if self.root.is_none() {
                    match self.review_gate() {
                        Ok(_) => {
                            self.review = None;
                            self.show_tab(bottom::BottomTab::Files);
                            self.nif_export_due = true;
                        }
                        Err(why) => {
                            self.status_line = format!("Export not started: {why}");
                            self.messages
                                .log(messages::Severity::Info, self.status_line.clone());
                        }
                    }
                    return;
                }
                // Never on a stale plan, whoever calls: wait again for the plan or the scan read in flight.
                if self.planning.is_some() || self.votes_pending.is_some() {
                    self.continue_after_plan = Some(how);
                    return;
                }
                // The decisions made in Review can change the blocks (a diffuse decided as PBR switches the set's
                // checks on), and an edit after the click can add a pitfall, so the guards and the gate run again
                // on the settled plan (audit 08f-2, Review Focus 1).
                if self.export_stopped() {
                    self.undo_half_step();
                    return;
                }
                // P9: `sdc.toml`'s writability again, once per Export click.
                let unwritable = self.also_toml_unwritable();
                self.refresh_half_unwritable();
                if let Some(r) = self.review.as_mut() {
                    r.toml_unwritable = unwritable;
                }
                match self.review_gate() {
                    Ok(_) => {
                        // Note FC2: the half-size answers are written here, on the settled plan with the sources
                        // checked; the export then waits for the plan that edit starts and comes back here.
                        if self.write_half_answers() {
                            self.continue_review();
                            return;
                        }
                        self.start_export_pass();
                        // Note FC5: Review's Export also writes the NIF edits, once the run has ended.
                        self.nif_export_queued = self.export.running.is_some();
                    }
                    Err(why) => {
                        self.undo_half_step();
                        self.status_line = format!("Export not started: {why}");
                        self.messages
                            .log(messages::Severity::Info, self.status_line.clone());
                    }
                }
            }
        }
    }

    /// Mark every item whose source file differs from the stamp the plan took (or, for a DDS input, from the stamp
    /// taken when it was listed). A packed output is checked against each of its sources, and a raw source feeding
    /// one is flagged with it. Returns the number of items flagged.
    pub fn check_sources(&mut self) -> usize {
        // A hand edit of a clean `sdc.toml` is simply taken (nothing to lose): the next plan reads it. With unsaved
        // edits the save dialog still asks (audit 2026-10-14 finding 3). Automatic types alone are recorded again
        // (Note FI4).
        let hand_edited = self.project.as_ref().is_some_and(|p| {
            (!p.is_dirty() || self.only_automatic_unsaved()) && p.changed_on_disk()
        });
        if hand_edited {
            if let Some(p) = self.project.as_mut() {
                p.reload();
            }
            self.drop_undo_note();
            self.history.clear();
            self.ignored_by_plan = false;
            self.refresh_banner();
            self.replan();
        }
        let mut changed_paths: std::collections::HashSet<PathBuf> = Default::default();
        for item in &self.items {
            let Some(job) = &item.job else {
                continue;
            };
            let stamps: Vec<(&std::path::Path, Option<SourceStamp>)> = match &job.pack {
                Some(pk) => pk
                    .sources
                    .iter()
                    .map(|s| (s.path.as_path(), s.stamp))
                    .collect(),
                None => vec![(job.input.as_path(), job.source)],
            };
            let rnm = job
                .active_rnm()
                .into_iter()
                .flat_map(|r| r.inputs(&job.profile.rnm))
                .map(|(f, _)| (f.path.as_path(), f.stamp));
            let stamps: Vec<_> = stamps.into_iter().chain(rnm).collect();
            for (path, planned) in stamps {
                if planned.is_some_and(|p| SourceStamp::of(path) != Some(p)) {
                    changed_paths.insert(path.to_path_buf());
                }
            }
        }
        let mut n = 0;
        for item in &mut self.items {
            let dds_changed = item
                .stamp
                .is_some_and(|s| SourceStamp::of(&item.input) != Some(s));
            item.changed_on_disk = dds_changed
                || changed_paths.contains(&item.input)
                || item.job.as_ref().is_some_and(|j| {
                    j.pack.as_ref().is_some_and(|pk| {
                        pk.sources.iter().any(|s| changed_paths.contains(&s.path))
                    }) || j.active_rnm().is_some_and(|r| {
                        r.inputs(&j.profile.rnm)
                            .iter()
                            .any(|(f, _)| changed_paths.contains(&f.path))
                    })
                });
            n += usize::from(item.changed_on_disk);
        }
        n + usize::from(hand_edited)
    }

    /// Plan again after sources changed on disk; the session cache reloads only the changed files.
    pub fn reload_changed(&mut self) {
        // Note FI1: the DDS tile images are read again with the reload.
        self.dds_thumbs.clear();
        self.dds_thumbs_asked.clear();
        self.dds_thumbs_failed.clear();
        self.replan();
    }

    /// Save first (the CLI command and the next session read `sdc.toml`), then run the export on a worker. One pass:
    /// the half-size answers were written before it, on the settled plan (Note FC2).
    fn start_export_pass(&mut self) {
        if self.export.running.is_some() {
            return;
        }
        // Note FC14: a failed save starts no export, so Review's *Half size* step for it is taken back.
        if self.save().is_err() {
            self.undo_half_step();
            return;
        }
        let (Some(session), Some(mut req)) = (self.session.clone(), self.request()) else {
            return;
        };
        self.nif_export_queued = false;
        req.on_existing = engine::api::OnExisting::IfChanged;
        let c = self.settings.checks;
        // Note FC2: Hold measures (an up-to-date output too) and writes the full chain, like Warn; nothing is held.
        req.low_detail = match (c.low_detail, c.drop_top_mip) {
            (false, _) => engine::api::LowDetailMode::Off,
            (true, true) => engine::api::LowDetailMode::Hold,
            (true, false) => engine::api::LowDetailMode::Warn,
        };
        let also = self.also_on();
        self.also_pass = also;
        self.also_error = None;
        self.also_written = None;
        self.also_report = None;
        // Final review I2: dropping an earlier export's sender releases its worker (no extra file).
        self.also_reply = None;
        self.also_json.clear();
        self.also_cancel = None;
        self.also_tail = None;
        self.pending_json = pbr_export::prepare(self);
        self.pending_cs = cs_pbr_export::prepare(self);
        self.pending_cm = cm_json::prepare(self);
        // R4: the ticks are for this export only. K12: the JSON answer goes with this export, or none does.
        self.pending_json_answer = self.review.take().and_then(|r| r.json_answer);
        self.review_json_cache.take();
        // FC7: this export does the re-encodes; the next plan says what is left.
        // shortcut: a cancelled export also drops the rows until the next plan; keep them if users miss them.
        self.recode.clear();
        // Note FC2: the stamps this export runs on, so a half-size row drops out once its source changes.
        let sources = self
            .items
            .iter()
            .filter_map(|i| Some((i.job_key.clone(), i.job.as_ref()?.source)))
            .collect();
        self.export = export::ExportModel {
            pace: Some(export::Pace::new(Instant::now())),
            sources,
            ..Default::default()
        };
        self.show_tab(bottom::BottomTab::Files);
        self.drawer_epoch += 1;
        let cancel = CancelToken::default();
        self.export.running = Some(cancel.clone());
        if also {
            // P8: one `drawer_epoch` for the three phases (bumped above); progress is the rows, counted model-side.
            // shortcut: the first phase's `BatchDone` stops the pace clock, so files/s and the ETA cover the PBR
            // phase only; upgrade to an ETA across phases if users ask.
            let spec = self.also_spec();
            crate::worker::spawn_export_also(self.tx.clone(), session, req, spec, cancel);
        } else {
            spawn_export(self.tx.clone(), session, req, cancel);
        }
    }

    /// The export ended: write the copies and the JSON of the sets that exported (spec §6.1). Nothing is written
    /// after a cancelled or failed export.
    fn finish_json(&mut self) {
        // K12: Review's answer is for this export only, whatever happens below.
        let answer = self.pending_json_answer.take();
        let Some(p) = self.pending_json.take() else {
            return;
        };
        let ok = self.export.error.is_none()
            && self
                .export
                .summary
                .as_ref()
                .is_some_and(|s| s.cancelled.is_empty());
        if !ok {
            let failed = self.export.error.is_some();
            if let Some(n) = pbr_export::unfinished_note(&p, failed) {
                self.export.notes.push(n);
            }
            return;
        }
        self.copy_files(&p);
        let root = p.root.clone();
        let key = history::record_key(&p.path);
        let shas = self
            .sets_doc
            .as_ref()
            .map(|d| history::records_at(&d.pgp_json_shas(), &root))
            .unwrap_or_default();
        let last = self.pgp_record(&root, &p.path);
        let named: Vec<String> = vec![p
            .path
            .strip_prefix(&root)
            .unwrap_or(&p.path)
            .to_string_lossy()
            .into_owned()];
        for old in history::orphans(&shas, &root, named.iter()) {
            self.export.notes.push(format!(
                "{}: the PGPatcher JSON is now {}; the old file was left in place and PGPatcher still reads it",
                old.display(),
                p.path.display()
            ));
        }
        match pbr_export::finish(
            &p,
            &self.export.rows,
            last.as_deref(),
            &mut self.export.notes,
        ) {
            pbr_export::Finish::Written(sha) => {
                self.also_json.push(p.path.clone());
                self.record_json_sha(&root, &key, &sha)
            }
            // C4: answered in Review for this very file (same path, same bytes); a file changed since asks again.
            pbr_export::Finish::Overwrite(text)
                if answer.as_ref().is_some_and(|a| {
                    a.path == p.path && a.sha.is_some() && history::sha256_of(&p.path) == a.sha
                }) =>
            {
                if answer.is_some_and(|a| a.overwrite) {
                    if let Some(sha) = pbr_export::overwrite(&p.path, &text, &mut self.export.notes)
                    {
                        self.also_json.push(p.path.clone());
                        self.record_json_sha(&root, &key, &sha);
                    }
                } else {
                    self.export.notes.push(format!(
                        "PGPatcher JSON kept as edited: {}",
                        p.path.display()
                    ));
                }
            }
            // Note FC5 (supersedes CA7's question after the export and BV10 08f-6's dialog half): Review waits for
            // the answer, so an edited file here is one with no answer for these bytes (changed during the export,
            // or exported without Review). It is kept, with a note; nothing asks after the export.
            pbr_export::Finish::Overwrite(_) => {
                let t = format!(
                    "PGPatcher JSON not written: {} was edited outside Tessalith and is kept; export again to choose",
                    p.path.display()
                );
                self.messages.notify(messages::Severity::Warning, t.clone());
                self.export.notes.push(t);
            }
            pbr_export::Finish::Nothing => {}
        }
    }

    /// The vanilla borrowed-slot copies, and their hash records (Note BV3).
    fn copy_files(&mut self, p: &pbr_export::PendingJson) {
        let Some(d) = self.sets_doc.as_ref() else {
            pbr_export::copy_files(
                p,
                &self.export.rows,
                &mut Default::default(),
                &mut self.export.notes,
            );
            return;
        };
        let old = d.copy_shas();
        let mut shas = old.clone();
        pbr_export::copy_files(p, &self.export.rows, &mut shas, &mut self.export.notes);
        if shas == old {
            return;
        }
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        let was_dirty = d.is_dirty();
        let side = d.set_copy_shas(&shas);
        if d.read_only() {
            self.export.notes.push(sidecar_only_note(&side));
            return;
        }
        self.save_hash_record(was_dirty);
    }

    /// The hash recorded for Tessalith's last PGPatcher JSON at `path` (#43: per name; a project from before
    /// per-name records has only the legacy one).
    fn pgp_record(&self, root: &std::path::Path, path: &std::path::Path) -> Option<String> {
        let d = self.sets_doc.as_ref()?;
        let shas = history::records_at(&d.pgp_json_shas(), root);
        if shas.is_empty() {
            d.last_json_sha256()
        } else {
            shas.get(&history::record_key(path)).cloned()
        }
    }

    /// Record the hash of the JSON just written and save `sdc-project.toml`, so the record matches the disk. While
    /// the project file is read-only (a bad `sdc-project.toml`, ruling P8) the JSON is still written but no hash is
    /// recorded: the next export then asks before overwriting it.
    fn record_json_sha(&mut self, root: &std::path::Path, key: &str, sha: &str) {
        let root = root.to_path_buf();
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        let was_dirty = d.is_dirty();
        // #43: one record per name; audit 08c-1: keyed by `record_key`, and kept when its file is gone.
        let mut shas = history::records_at(&d.pgp_json_shas(), &root);
        shas.insert(key.to_string(), sha.to_string());
        let side = d.set_last_json_sha256(sha).and(d.set_pgp_json_shas(&shas));
        // Note FC11 (supersedes P21's next-export ask): a read-only document still gets the sidecar's record.
        if d.read_only() {
            self.export.notes.push(sidecar_only_note(&side));
            return;
        }
        self.save_hash_record(was_dirty);
    }

    /// Saves `sdc-project.toml` after a hash was recorded, unless it already held unsaved edits (set edits and
    /// Undo stay live while an export runs; Note BD2): those are the user's to save or discard, so the record waits
    /// for the next Save. The hash is also recorded at write time, so the next export does not ask (Note FC11).
    fn save_hash_record(&mut self, was_dirty: bool) {
        if self.save_record(was_dirty) {
            const NOTE: &str = "hash recorded; written with the next Save";
            // Up to three hash records per export (PGPatcher, CS PBR, complex material): one note.
            if !self.export.notes.iter().any(|n| n == NOTE) {
                self.export.notes.push(NOTE.into());
            }
        }
    }

    /// The one save-if-clean rule for records kept outside the undo history (JSON hashes, generated-file
    /// provenance; P14): saves `sdc-project.toml` when it had no unsaved edits, else leaves the record for the next
    /// Save. True when it deferred.
    pub(crate) fn save_record(&mut self, was_dirty: bool) -> bool {
        if was_dirty {
            return true;
        }
        let Some(d) = self.sets_doc.as_mut() else {
            return false;
        };
        if let Err(e) = d.save(false) {
            self.hash_record_failed(&e);
        }
        false
    }

    /// A JSON hash record that did not save (audit item 16): the save question (Overwrite / Reload for a file
    /// changed on disk), unless a dialog is open; an open question must stay, so the error goes to the notes then.
    fn hash_record_failed(&mut self, e: &SaveError) {
        if self.dialog.is_some() {
            self.export
                .notes
                .push(format!("{}: {e}", DocKind::Project.file()));
        } else {
            self.save_failed(DocKind::Project, e);
        }
    }

    /// The export ended: do what was asked while it ran (close, or open another folder). True when there was
    /// something to do (only an answered question counts).
    fn run_after_export(&mut self) -> bool {
        // A question still unanswered when the export ends by itself is dropped, not assumed to be a yes: the window
        // stays, and the user sees the report.
        if matches!(self.dialog, Some(Dialog::ExportRunning(_))) {
            self.dialog = None;
            return false;
        }
        let action = self.after_export.take();
        match action {
            None => false,
            Some(AfterExport::Close) => {
                self.close_ready = true;
                true
            }
            Some(AfterExport::Open(root)) => {
                self.request_open(root);
                true
            }
        }
    }

    /// True once, when the window may close because the export that was in its way has ended. The window then asks
    /// `request_close` again (unsaved edits still ask).
    pub fn take_close_ready(&mut self) -> bool {
        std::mem::take(&mut self.close_ready)
    }

    /// The answer to `Dialog::ExportRunning`: cancel the export and do `action` when it has ended, or keep it running.
    pub fn resolve_export_running(&mut self, action: AfterExport, cancel_and_continue: bool) {
        self.dialog = None;
        if !cancel_and_continue {
            // "Keep running" withdraws any earlier "cancel and …": the user now wants the export to finish and
            // nothing to happen after it (spec §8).
            self.after_export = None;
            return;
        }
        // A newer answer replaces an older one; it never stacks.
        self.after_export = Some(action);
        // Note FC2 (supersedes CL27 M2): there is no held pass to wait for; Stop cancels at the next file.
        if self.export.running.is_some() {
            self.cancel_export();
        } else {
            // It ended while the dialog was up.
            self.run_after_export();
        }
    }

    pub fn cancel_export(&mut self) {
        if let Some(c) = &self.export.running {
            c.cancel();
        }
    }

    /// Select the texture whose output is `job_key`; the centre keeps showing whatever it shows.
    pub fn select_job(&mut self, job_key: &str) {
        if let Some(k) = self
            .items
            .iter()
            .find(|i| i.job_key == job_key)
            .map(|i| i.key.clone())
        {
            self.select_item(&k);
        }
    }

    fn apply_plan(&mut self, view: PlanView) {
        // A plan landed: any blame on `sdc.toml` is settled, so a later failure must not unblame it (review I1 of #9).
        self.ignored_by_plan = false;
        // Task 9 (Z11): the sub-types before this plan chain, compared when it settles.
        if self.type_base.is_none() {
            self.type_base = Some(self.subtypes());
        }
        // Look the expanded item's set up before the items are replaced: if the item vanishes, its set is the
        // fallback selection (Review Focus 1).
        let expanded_set = self
            .expanded
            .as_deref()
            .and_then(|k| self.item(k))
            .and_then(|i| i.set_key.clone());
        let selected_expanded = self.expanded.is_some()
            && self.selection == Selection::Item(self.expanded.clone().unwrap_or_default());
        self.target = view.target;
        self.items = view.items;
        self.refresh_game_paths();
        self.request_dds_thumbs();
        // K1: the checks run on this plan's analyses at the end of the poll that lands it.
        self.check_state.plans_applied += 1;
        self.invalidate_views();
        self.sets = view.sets;
        self.sets_root = Some(view.output_root);
        self.unmatched_keys = view.unmatched;
        self.rnm_notes = view.rnm_notes;
        self.rnm_keys = view.rnm_keys;
        self.batch_preset = view.preset_id;
        self.loaded_user_presets = view.user_presets;
        self.skipped_user_presets = view.skipped_user_presets;
        self.unreadable = view.unreadable;
        self.recode = view.recode;
        self.stale = view.stale;
        // The JSON rows are added once the plan settles (`finish_plan`): they read the settled sets.
        self.stale_json.clear();
        // The count of undecided textures is shown (amber) in the Textures column header.
        self.status_line = self.counts_line();
        // The undo note stays through the plan the undo started, and goes with the next one.
        if matches!(self.undo_note_at, Some((_, true))) {
            self.drop_undo_note();
        }
        if let Some((_, shown)) = self.undo_note_at.as_mut() {
            *shown = true;
        }
        for n in [&self.status_note, &self.undo_note].into_iter().flatten() {
            self.status_line = format!("{n} {}", self.status_line);
        }
        self.prune_selection();
        // Keep the selection when its item or set still exists; else nothing.
        let gone = match &self.selection {
            Selection::Item(k) => self.item(k).is_none(),
            Selection::Set(k) => !self.sets.iter().any(|s| &s.key == k),
            Selection::None => false,
        };
        if gone {
            self.selection = Selection::None;
        }
        // Generate's first file, by the plan Generate started (it lists the file); never a later one.
        if let Some(k) = self.gen_select.take().filter(|k| self.item(k).is_some()) {
            self.select_item(&k);
        }
        if let Some(k) = self.expanded.clone() {
            if self.item(&k).is_none() {
                self.expanded = None;
                self.preview_close();
                if selected_expanded {
                    if let Some(set) =
                        expanded_set.filter(|s| self.sets.iter().any(|x| &x.key == s))
                    {
                        self.selection = Selection::Set(set);
                    }
                }
            } else {
                self.preview_after_plan();
            }
        }
        // Output paths depend on this plan's sub-types (DECISIONS P1): plan once more when they changed. Bounded.
        // `export_after_plan` survives the return: export starts when the final plan lands.
        self.refresh_mesh_votes(true);
    }

    /// Note FI1: a DDS tile with no image takes the cached one when its stamp matches; the rest are decoded on one
    /// worker (none asked twice while out).
    fn request_dds_thumbs(&mut self) {
        let mut ask = vec![];
        for item in self.items.iter_mut() {
            let Some(stamp) = item
                .stamp
                .filter(|_| item.kind == ItemKind::Dds && item.thumb.is_none())
            else {
                continue;
            };
            match self.dds_thumbs.get(&item.input) {
                Some((s, t)) if *s == stamp => item.thumb = Some(t.clone()),
                _ if self.dds_thumbs_asked.get(&item.input) == Some(&stamp)
                    || self.dds_thumbs_failed.get(&item.input) == Some(&stamp) => {}
                _ => ask.push((item.input.clone(), stamp)),
            }
        }
        let Some(session) = self.session.as_ref().filter(|_| !ask.is_empty()) else {
            return;
        };
        self.dds_thumb_requests += ask.len();
        self.dds_thumbs_asked.extend(ask.iter().cloned());
        spawn_dds_thumbs(self.tx.clone(), ask, session.bc1_model());
    }

    /// Note FI1: a DDS tile image landed. True when an item took it (the caller invalidates the views once). An
    /// error keeps only the refused stamp (not read again until the file changes): the tile stays grey and the
    /// preview gives the reason.
    fn dds_thumb_landed(
        &mut self,
        path: PathBuf,
        stamp: SourceStamp,
        thumb: Result<Arc<Thumbnail>, String>,
    ) -> bool {
        if self.dds_thumbs_asked.get(&path) == Some(&stamp) {
            self.dds_thumbs_asked.remove(&path);
        }
        let Ok(thumb) = thumb else {
            self.dds_thumbs_failed.insert(path, stamp);
            return false;
        };
        let mut took = false;
        for item in self
            .items
            .iter_mut()
            .filter(|i| i.kind == ItemKind::Dds && i.input == path && i.stamp == Some(stamp))
        {
            item.thumb = Some(thumb.clone());
            took = true;
        }
        if took {
            self.dds_thumbs.insert(path, (stamp, thumb));
        }
        took
    }

    /// Note FI1: DDS tile images are still being decoded (the window keeps polling).
    pub fn dds_thumbs_pending(&self) -> bool {
        !self.dds_thumbs_asked.is_empty()
    }

    /// The tail of `apply_plan`, once the scan-database evidence of this plan is in.
    fn finish_plan(&mut self) {
        let now = EngineInputs::of(self);
        if self.last_inputs.as_ref() != Some(&now) && self.auto_replans < 2 {
            self.auto_replans += 1;
            self.replan();
            return;
        }
        self.auto_replans = 0;
        self.refresh_stale_json();
        // Task 9 (Z11): the final pass; a cleanup re-plans once.
        if self.settle_types() {
            return;
        }
        if self.follow_weighting() {
            return;
        }
        // Note FC1: the settled plan's own decisions go into the project; its re-plan finds nothing new to record. A
        // queued export waits for that plan (the flags below are taken then).
        if self.record_auto_decisions() {
            return;
        }
        // Both flags are taken together so a leftover one cannot fire on a later, unrelated plan; the export the
        // user clicked wins.
        let (clicked, continued) = (
            std::mem::take(&mut self.export_after_plan),
            self.continue_after_plan.take(),
        );
        match continued {
            // Audit 08f-1 holds for Review's queued Export too: never over an open question.
            _ if (clicked || continued.is_some()) && self.dialog.is_some() => {
                self.queued_export_dropped()
            }
            _ if clicked => self.export_checks(),
            Some(how) => self.run_continue(how),
            None => {}
        }
    }

    /// Audit 08f-1: an export queued behind a plan never starts over an open question (its answer could open
    /// another folder mid-export); the user clicks Export again after answering.
    fn queued_export_dropped(&mut self) {
        let note = "Export not started: answer the open dialog, then click Export again";
        self.status_line = note.into();
        self.messages.notify(messages::Severity::Warning, note);
        // Note FC2: a half size written for this export goes with it.
        self.undo_half_step();
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.poll();
            if !self.busy() && !self.v2_jobs_pending() {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// True while a scan-database read is in flight (tests).
    pub fn votes_pending(&self) -> bool {
        self.votes_pending.is_some()
    }

    /// The plan generation, bumped once per `replan` (tests: one plan per bulk apply).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn items_rev(&self) -> u64 {
        self.items_rev
    }

    /// Note FI15: every item's output game path (a skipped DDS included), from its `textures/` component on, with
    /// `/`, lowercased by the engine's output-key fold (equal to the ASCII fold for ASCII paths), sorted and
    /// deduplicated; paths with no `textures/` component are left out. The `u64` changes only when the list does,
    /// so the Meshes view can key a cache on it.
    pub fn output_game_paths(&self) -> (u64, &[String]) {
        (self.game_paths.0, &self.game_paths.1)
    }

    /// Rebuilt once per landed plan. `job_key` is the engine's output key: root-relative, `/`, lowercased.
    // shortcut: the engine key folds Unicode, so a non-ASCII folder name may not match the NIF side's ASCII fold;
    // carry an ASCII-folded game path on TextureItem if a non-ASCII path is ever reported.
    fn refresh_game_paths(&mut self) {
        let mut paths: Vec<String> = self
            .items
            .iter()
            .filter_map(|i| {
                let k = &i.job_key;
                let at = if k.starts_with("textures/") {
                    0
                } else {
                    k.find("/textures/")? + 1
                };
                Some(k[at..].to_string())
            })
            .collect();
        paths.sort();
        paths.dedup();
        if paths != self.game_paths.1 {
            self.game_paths = (self.game_paths.0 + 1, paths);
        }
    }

    /// Call after writing `items` or `sets` in place (tests); a plan does it itself.
    pub fn invalidate_views(&mut self) {
        self.items_rev += 1;
        self.item_index = self
            .items
            .iter()
            .enumerate()
            .map(|(n, i)| (i.key.clone(), n))
            .rev()
            .collect();
    }

    /// Replace the Tree_Anim scan evidence; the JSON editor's cache sees it (DECISIONS AG109).
    pub fn set_tree_anim(&mut self, t: std::collections::BTreeMap<String, bool>) {
        if self.mesh_tree_anim != t {
            self.mesh_tree_anim = t;
            self.evidence_rev += 1;
        }
    }

    /// Replace the scan's known paths; the JSON editor's cache sees it (DECISIONS AG109).
    pub fn set_known(&mut self, k: known_paths::KnownPaths) {
        if *self.known != k {
            self.known = Arc::new(k);
            self.evidence_rev += 1;
        }
    }

    /// What the PGPatcher JSON editor's text and issues are built from (DECISIONS AG109): the set-view key, the
    /// mesh evidence the auto flags read, and the file name.
    pub(crate) fn json_key(&self) -> (ViewKey, u64, String) {
        (
            self.view_key(),
            self.evidence_rev,
            crate::model::pbr_export::json_name(self),
        )
    }

    /// Everything `sets::views` and `pbr_export::export_blocks` read (DECISIONS Note Y7).
    pub(crate) fn view_key(&self) -> ViewKey {
        ViewKey {
            items: self.items_rev,
            inputs: self.inputs_rev,
            doc: self
                .sets_doc
                .as_ref()
                .map_or(0, crate::sets_doc::SetsDoc::rev),
            root: self.root.clone(),
            // Note CN: the PBR folder while a tick is on, so the JSON paths follow a tick at once.
            export_root: self.output_root(),
            target: self.target,
            scan_db: self.scan_db.clone(),
            scan: self.scan_stamp,
            zip: self.zip_on(),
        }
    }

    pub fn item(&self, key: &str) -> Option<&TextureItem> {
        self.item_index
            .get(key)
            .and_then(|&i| self.items.get(i))
            .filter(|i| i.key == key)
    }

    pub fn undecided(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::NeedsDecision)
            .count()
    }

    /// End the multi-selection; the revision keeps counting up.
    fn reset_multi(&mut self) {
        self.multi = bulk::MultiSelection {
            selection_rev: self.multi.selection_rev + 1,
            ..Default::default()
        };
    }

    /// Select one texture; any multi-selection ends (F3a D1).
    pub fn select_item(&mut self, key: &str) {
        self.selection = Selection::Item(key.to_string());
        self.reset_multi();
        self.bulk = Default::default();
        // Note FB7: also a click on the texture already selected brings it to the front of the centre.
        self.centre_tab = CentreTab::Selection;
    }

    /// Select one set; any multi-selection ends (F3a D1).
    pub fn select_set(&mut self, key: &str) {
        self.selection = Selection::Set(key.to_string());
        self.reset_multi();
        self.bulk = Default::default();
        // Note FB7: also a click on the set already selected brings it to the front of the centre.
        self.centre_tab = CentreTab::Selection;
    }

    pub fn expand(&mut self, key: &str) {
        self.select_item(key);
        self.expanded = Some(key.to_string());
        self.view.mip = 0;
        self.preview_open(key);
    }

    pub fn collapse(&mut self) {
        self.expanded = None;
        self.preview_close();
        self.overlay.clear();
    }

    fn snapshot_of(
        p: Option<&ProjectDoc>,
        s: Option<&crate::sets_doc::SetsDoc>,
    ) -> history::Snapshot {
        history::Snapshot {
            sdc_toml: p.map(ProjectDoc::text).unwrap_or_default(),
            project: s.map(crate::sets_doc::SetsDoc::text).unwrap_or_default(),
        }
    }

    /// Each six-file cube item's key with the `[files]` keys of its other five faces, from the plan's cube sources.
    fn cube_face_keys(&self) -> Vec<(String, Vec<String>)> {
        let Some(root) = self.root.as_deref() else {
            return vec![];
        };
        self.items
            .iter()
            .filter_map(|i| {
                let c = i.job.as_ref()?.cube.as_ref()?;
                (c.sources.len() == 6).then(|| {
                    let others = c.sources.iter().map(|s| items::key_of(root, &s.path));
                    (i.key.clone(), others.filter(|k| *k != i.key).collect())
                })
            })
            .collect()
    }

    /// Every edit goes through here (spec §7.2): run `f` on both documents, record one undo step when anything
    /// changed, and plan again when `replan` (DECISIONS P7: JSON-only edits do not). A read-only document ignores
    /// its own edits (its text does not change, so it adds nothing to the step); the other document is unaffected
    /// (DECISIONS P8).
    pub fn edit_docs(
        &mut self,
        label: &str,
        merge: Option<String>,
        replan: bool,
        f: impl FnOnce(&mut ProjectDoc, &mut crate::sets_doc::SetsDoc),
    ) {
        // DT31: a six-file cube's faces must resolve alike (CUBE-FACES), so whatever an edit does to the item's own
        // entry (its +X face) is repeated on the other five face entries, inside the same step.
        let cubes = self.cube_face_keys();
        let f = move |p: &mut ProjectDoc, s: &mut crate::sets_doc::SetsDoc| {
            let before: Vec<_> = cubes.iter().map(|(k, _)| p.entry_fields(k)).collect();
            f(p, s);
            for ((k, others), b) in cubes.iter().zip(&before) {
                p.copy_entry_changes(k, b, others);
            }
        };
        let (Some(p), Some(s)) = (self.project.as_mut(), self.sets_doc.as_mut()) else {
            return;
        };
        // A batch records one step around all its writes (`batch_edit`): no snapshot per write.
        if self.batch {
            return f(p, s);
        }
        let before = Self::snapshot_of(Some(p), Some(s));
        let dirty = (p.is_dirty(), s.is_dirty());
        f(p, s);
        let after = Self::snapshot_of(Some(p), Some(s));
        if after == before {
            // An equal write is no edit: `touch` marked the document dirty, so undo that (trace #28).
            p.set_dirty(dirty.0);
            s.set_dirty(dirty.1);
            return;
        }
        self.drop_undo_note();
        self.history.push(history::Command {
            label: label.into(),
            before,
            after,
            merge,
            created_file: None,
        });
        self.auto_replans = 0;
        if replan {
            self.replan_after_edit();
        }
    }

    /// The BC7 weighting row's two fields for pick `w` over `profile` (`fields::weighting_writes`), as one step.
    fn write_weighting(
        &mut self,
        item_key: &str,
        w: &str,
        profile: &engine::config::Profile,
        merge: String,
    ) {
        let (k, w, profile) = (item_key.to_string(), w.to_string(), profile.clone());
        self.edit_docs("Change BC7 weighting", Some(merge), true, move |p, _| {
            let (own_p, own_w) = (
                p.file_value(&k, "encoder.perceptual"),
                p.file_value(&k, fields::WEIGHTING),
            );
            let writes = fields::weighting_writes(&w, &profile, own_p.as_ref(), own_w.as_ref());
            for (f, v) in [
                ("encoder.perceptual", writes.0),
                (fields::WEIGHTING, writes.1),
            ] {
                match v {
                    Some(v) => p.set_file_value(&k, f, &v),
                    None => p.clear_file_value(&k, f),
                }
            }
        });
    }

    /// Note FC8 (CP10): once the plan after a weighting pick settles, the profile shows what the cleared own value
    /// hid; when the row is still not the pick, the pick is written again over it, joined to the pick's step. True
    /// when that wrote (a re-plan follows).
    fn follow_weighting(&mut self) -> bool {
        let Some((k, w, merge)) = self.weighting_follow.take() else {
            return false;
        };
        let Some(profile) = self
            .item(&k)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.clone())
        else {
            return false;
        };
        if fields::weighting_value(&profile) == json!(w) {
            return false;
        }
        let pushes = self.history.pushes;
        self.write_weighting(&k, &w, &profile, merge);
        self.history.pushes != pushes
    }

    /// An `sdc.toml` edit (a read-only `sdc.toml` ignores it), re-planned. The sets document is not touched, so
    /// it works the same when `sdc-project.toml` is read-only.
    fn edit(&mut self, label: &str, f: impl FnOnce(&mut ProjectDoc)) {
        if self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return;
        }
        self.edit_docs(label, None, true, |p, _| f(p));
    }

    fn apply_snapshot(&mut self, s: &history::Snapshot) {
        self.dropped_fills.clear();
        // Note FC1: the row's Undo is for its own step; `pushes` does not move on an undo.
        self.auto_note = None;
        // Note FB3: the bulk result line describes the edit that is now taken back (or redone).
        self.bulk.report = None;
        // Task 9 (Z11): restoring a state as it was is no sub-type change to clean up after.
        self.reset_type_follow();
        // Note FC8 ledger minor: a weighting re-write queued for the plan of the step undone no longer applies.
        self.weighting_follow = None;
        self.reencode_follow = None;
        // Only a document the step changed is restored: `restore` marks it dirty, and an untouched one must stay clean
        // (no empty sdc-project.toml on Save, no spurious changed-on-disk dialog).
        if let Some(p) = self.project.as_mut().filter(|p| p.text() != s.sdc_toml) {
            p.restore(&s.sdc_toml);
        }
        // The JSON hash is not part of a step: export writes it outside history (spec §7.2, final review I2).
        if let Some(d) = self.sets_doc.as_mut() {
            d.restore_keeping_json_sha(&s.project);
        }
        // An undo or redo changes the project like an edit (AF11), without the edit's type-follow bookkeeping.
        self.replan_changed();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    /// The label of the step Undo would take back, for the button's hover text.
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo.last().map(|c| c.label.as_str())
    }
    /// The label of the step Redo would re-apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo.last().map(|c| c.label.as_str())
    }

    /// Remove the "Undid/Redid" note from the status line (an edit, or a new history).
    fn drop_undo_note(&mut self) {
        self.undo_note_at = None;
        if let Some(n) = self.undo_note.take() {
            self.status_line = self.status_line.replacen(&format!("{n} "), "", 1);
            if self.status_line == n {
                self.status_line.clear();
            }
        }
    }

    /// The note is shown for at most [`UNDO_NOTE_FOR`] (the UI's per-frame `tick`).
    pub(crate) fn expire_undo_note(&mut self, now: Instant) {
        if let Some((at, _)) = self.undo_note_at {
            if now.saturating_duration_since(at) >= UNDO_NOTE_FOR {
                self.drop_undo_note();
            }
        }
    }

    /// How long the undo note has left at `now`: the app asks for a frame then, so an idle window drops it on time.
    pub fn undo_note_left(&self, now: Instant) -> Option<Duration> {
        let (at, _) = self.undo_note_at?;
        Some(UNDO_NOTE_FOR.saturating_sub(now.saturating_duration_since(at)))
    }

    /// "1 texture, 1 set" / "3 textures, 2 sets" (Note FI14).
    fn counts_line(&self) -> String {
        let (t, s) = (self.items.len(), self.sets.len());
        format!(
            "{t} texture{}, {s} set{}",
            review::plural(t),
            review::plural(s)
        )
    }

    /// Show "Undid: <label>" (or "Redid") in the status line until the next edit. It stands beside an Export-cancelled
    /// note (AF10, AF11), which stays; the encoding note went with the undo's `reset_type_follow`.
    fn undo_noted(&mut self, verb: &str, label: &str) {
        self.drop_undo_note();
        let n = format!("{verb}: {label}.");
        // In front of the normal status text, not in place of it; the plan the step started rewrites the counts.
        let counts = self.counts_line();
        self.status_line = match &self.status_note {
            Some(c) => format!("{n} {c} {counts}"),
            None => format!("{n} {counts}"),
        };
        self.messages.notify(messages::Severity::Info, n.clone());
        self.undo_note = Some(n);
        self.undo_note_at = Some((Instant::now(), false));
    }

    /// Undo one step. A step that created a file (Duplicate source) deletes it first, and only if it is unchanged
    /// (spec §3.4); otherwise the undo is refused with the reason.
    pub fn undo(&mut self) {
        let Some(c) = self.history.undo_step() else {
            return;
        };
        if let Some(cf) = &c.created_file {
            if let Err(why) = remove_created(cf) {
                self.history.refuse_undo();
                self.dialog = Some(Dialog::Error(why));
                return;
            }
        }
        // Note FC12: the step the plan rejected `sdc.toml` after: its document comes back (read-only no more), and
        // the step's `before` is restored into it. A document rejected when it loaded has no such step.
        if c.label.contains(REJECTED) {
            // Note FC14: a Redo re-applies the edit, not the old blame.
            if let Some(r) = self.history.redo.last_mut() {
                if let Some(i) = r.label.find(REJECTED) {
                    r.label.truncate(i);
                }
            }
            if let Some(p) = self.project.as_mut().filter(|p| p.blamed()) {
                p.unblame();
                self.ignored_by_plan = false;
                self.refresh_banner();
            }
        }
        self.apply_snapshot(&c.before);
        self.undo_noted("Undid", &c.label);
    }

    /// Note FB5: after the documents load, each one's crash `.recovery` file is loaded into it as one Undo step
    /// (`Restore crash edits`) and a toast says so. An empty or equal file is deleted; a read-only document is left
    /// alone (the banner names its file). The file itself goes only when the user saves.
    pub fn apply_recovery(&mut self) {
        use crate::crash_log::{discard_recovery, recovery_text, recovery_text_held, Recovery};
        const LABEL: &str = "Restore crash edits";
        self.crash_restore = None;
        let docs = [
            self.project
                .as_ref()
                .map(|p| (p.path().to_path_buf(), p.text(), p.read_only())),
            self.sets_doc
                .as_ref()
                .map(|s| (s.path().to_path_buf(), s.text(), s.read_only())),
        ];
        let mut found: [Option<(std::path::PathBuf, String)>; 2] = [None, None];
        for (slot, doc) in found.iter_mut().zip(docs) {
            let Some((path, doc_text, read_only)) = doc else {
                continue;
            };
            if read_only {
                if matches!(recovery_text(&path, Some(&doc_text)), Recovery::Pending(_)) {
                    self.held_recovery.insert(path);
                }
                continue;
            }
            let held = self.held_recovery.remove(&path);
            match recovery_text_held(&path, Some(&doc_text), held) {
                Recovery::Pending(t) => *slot = Some((path, t)),
                Recovery::Empty => discard_recovery(&path),
                Recovery::None => {}
            }
        }
        if found.iter().all(Option::is_none) {
            return;
        }
        let [project, sets] = found;
        let before = self.history.undo.len();
        // Only a document whose text really changed counts as restored: `restore` ignores text that does not parse.
        let mut restored = vec![];
        self.edit_docs(LABEL, None, false, |p, s| {
            if let Some((path, t)) = project {
                let old = p.text();
                p.restore(&t);
                if p.text() != old {
                    restored.push(path);
                }
            }
            if let Some((path, t)) = sets {
                let old = s.text();
                s.restore(&t);
                if s.text() != old {
                    restored.push(path);
                }
            }
        });
        // shortcut: a file that does not parse as TOML restores nothing, shows no notice and stays on disk (the
        // other document's file, if it restored, is the only one the toast's Undo deletes); give it a notice if one
        // is ever seen.
        if self.history.undo.len() > before {
            self.crash_restore = Some((self.history.undo.len(), self.history.pushes, restored));
            self.messages.notify_with(
                messages::Severity::Info,
                "Restored unsaved edits from the crash",
                Some(messages::ToastAction::UndoCrashRestore),
            );
        }
    }

    /// Note FB5: the crash-restore step is the top of the undo stack: at its depth, with nothing pushed since (an
    /// undo and redo of it push nothing).
    pub(crate) fn crash_restore_on_top(&self) -> bool {
        self.crash_restore
            .as_ref()
            .is_some_and(|(depth, pushes, _)| {
                self.history.undo.len() == *depth && self.history.pushes == *pushes
            })
    }

    /// The restore toast's Undo (Note FB5): takes back the restore step and deletes the `.recovery` files, since the
    /// user rejected them. Once another edit sits on top of the step it only says so and changes nothing. (A plain
    /// Ctrl+Z keeps the files.)
    pub fn undo_crash_restore(&mut self) {
        let Some((_, _, files)) = self.crash_restore.as_ref() else {
            return;
        };
        let files = files.clone();
        if !self.crash_restore_on_top() {
            self.messages.notify(
                messages::Severity::Info,
                "The restored edits were already undone or edited since: use Undo or Redo",
            );
            return;
        }
        self.crash_restore = None;
        self.undo();
        for f in files {
            crate::crash_log::discard_recovery(&f);
        }
    }

    /// Redo one step. A step that created a file copies it again from its source.
    pub fn redo(&mut self) {
        let Some(c) = self.history.redo_step() else {
            return;
        };
        if let Some(cf) = &c.created_file {
            match copy_again(cf) {
                Ok(sha256) => {
                    // Undo deletes the copy only when it is still this one (audit item 8).
                    if let Some(f) = self
                        .history
                        .undo
                        .last_mut()
                        .and_then(|s| s.created_file.as_mut())
                    {
                        f.sha256 = sha256;
                    }
                }
                Err(why) => {
                    self.history.refuse_redo();
                    self.dialog = Some(Dialog::Error(why));
                    return;
                }
            }
        }
        self.apply_snapshot(&c.after);
        self.undo_noted("Redid", &c.label);
    }

    /// The slot card's preset dropdown: format and tag together, one undo step (M3b §4.1).
    pub fn set_format(
        &mut self,
        item: &str,
        f: engine::config::Format,
        tag: engine::config::ColorTag,
    ) {
        let k = item.to_string();
        self.edit("Format", move |p| {
            p.set_file_value(&k, "format", &json!(f));
            p.set_file_value(&k, "color.tag", &json!(tag));
            // NVTT encodes BC7 and BC6H only: a texture leaving them goes back to the built-in encoder.
            if !engine::policy::backend_fits(engine::config::Backend::Nvtt, f) {
                p.clear_file_value(&k, "encoder.backend");
            }
            // Note CP: BC7 weights mean nothing on another format (the row hides there), so they go with it.
            if f != engine::config::Format::Bc7 {
                p.clear_file_value(&k, fields::WEIGHTING);
            }
        });
    }

    /// The Diffuse card's Opaque checkbox: alpha `remove` while ticked; unticked returns to the resolved default.
    pub fn set_opaque(&mut self, item: &str, on: bool) {
        let k = item.to_string();
        self.edit("Opaque", move |p| {
            if on {
                p.set_file_value(&k, "alpha.policy", &json!(AlphaPolicy::Remove));
            } else {
                p.clear_file_value(&k, "alpha.policy");
            }
        });
    }

    pub fn is_opaque(&self, item: &str) -> bool {
        self.project
            .as_ref()
            .and_then(|p| p.file_value(item, "alpha.policy"))
            .is_some_and(|v| v == json!(AlphaPolicy::Remove))
    }

    pub fn set_field(&mut self, item_key: &str, field: &str, value: serde_json::Value) {
        if field == "subtype" {
            if let Ok(st) = serde_json::from_value::<Subtype>(value.clone()) {
                return self.edit_subtype("Change subtype", item_key, Some(st));
            }
        }
        // Note CP: the BC7 weighting row writes two fields (`fields::weighting_writes`, CP10).
        if field == fields::WEIGHTING {
            let w = value.as_str().unwrap_or("");
            if w != "perceptual"
                && serde_json::from_value::<engine::config::Bc7Weights>(value.clone()).is_err()
            {
                return;
            }
            let Some(profile) = self
                .item(item_key)
                .and_then(|i| i.job.as_ref())
                .map(|j| j.profile.clone())
            else {
                return;
            };
            if self.project.as_ref().is_none_or(ProjectDoc::read_only) {
                return;
            }
            // Note FC8 (supersedes CP10's second pick): an own `perceptual = true` or own weighting may hide an
            // inherited value the profile cannot show; the settled plan completes the pick (`follow_weighting`).
            let hides = self.project.as_ref().is_some_and(|p| {
                p.file_value(item_key, "encoder.perceptual") == Some(json!(true))
                    || p.file_value(item_key, fields::WEIGHTING)
                        .is_some_and(|v| v != json!("linear"))
            });
            // Unique per pick, so two picks stay two steps; the follow-up joins this one.
            let merge = format!("bc7-weighting#{}", self.history.pushes);
            self.write_weighting(item_key, w, &profile, merge.clone());
            self.weighting_follow = hides.then(|| (item_key.to_string(), w.to_string(), merge));
            return;
        }
        // Note CL (M9): an unticked half size removes the key; a written `false` would stay as a provenance entry.
        if field == "drop_top_mip" && value == json!(false) {
            return self.clear_field(item_key, field);
        }
        // M6: never a half size where there is no mip to drop (the row is disabled there).
        let locked = |m: &Self| {
            m.item(item_key)
                .and_then(|i| i.job.as_ref())
                .and_then(|j| fields::field_lock(m.target, &j.profile, field))
                .is_some()
        };
        if field == "drop_top_mip" && locked(self) {
            return;
        }
        // DECISIONS Note NV: "Built-in" is the texture's own format default, so one bulk value fits BC7 and BC6H.
        let value = match field {
            "encoder.backend" if value != serde_json::json!(engine::config::Backend::Nvtt) => self
                .item(item_key)
                .and_then(|i| i.job.as_ref())
                .map_or(value, |j| {
                    serde_json::json!(engine::policy::default_backend(j.profile.format))
                }),
            _ => value,
        };
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(&format!("Change {f}"), move |p| {
            p.set_file_value(&k, &f, &value)
        });
    }

    pub fn clear_field(&mut self, item_key: &str, field: &str) {
        if field == "subtype" {
            return self.edit_subtype("Reset subtype", item_key, None);
        }
        let k = item_key.to_string();
        let f = field.to_string();
        self.edit(&format!("Reset {f}"), move |p| {
            p.clear_file_value(&k, &f);
            // The Format row writes the tag with the format (`set_format`), so its reset clears both.
            if f == "format" {
                p.clear_file_value(&k, "color.tag");
            }
            // The BC7 weighting row stands for both of its fields (Note CP).
            if f == fields::WEIGHTING {
                p.clear_file_value(&k, "encoder.perceptual");
            }
        });
    }

    /// A confirmation writes the value explicitly, or adds the code to the file's `allow` (M3a spec §5.2).
    pub fn decide(&mut self, item_key: &str, answer: Answer) {
        if let Answer::Subtype(st) = answer {
            return self.edit_subtype("Decide", item_key, Some(st));
        }
        let k = item_key.to_string();
        self.edit("Decide", move |p| match answer {
            Answer::Subtype(st) => p.set_file_value(&k, "subtype", &json!(st)),
            Answer::Alpha(a) => p.set_file_value(&k, "alpha.policy", &json!(a)),
            Answer::Green(g) => p.set_file_value(&k, "channels.green_convention", &json!(g)),
            Answer::Acknowledge(c) => {
                let mut codes = p.allow(&k);
                codes.insert(c);
                p.set_allow(&k, &codes);
            }
        });
    }

    /// The PGPatcher JSON file name (header field); empty restores the default (the folder's name). No re-plan (P7).
    pub fn set_json_name(&mut self, name: &str) {
        let n = name.trim().to_string();
        self.edit_docs("JSON name", Some("json_name".into()), false, move |_, s| {
            s.set_json_name(if n.is_empty() { None } else { Some(&n) })
        });
    }

    /// The preset creator's model over `user_presets` (`None` in tests: no folder, so nothing is ever written).
    pub fn presets_model(&mut self) -> &mut presets::PresetsModel {
        let (dir, target) = (self.user_presets.clone(), self.target);
        let pm = self
            .presets
            .get_or_insert_with(|| presets::PresetsModel::new(dir, target));
        pm.fallback = target;
        pm
    }

    /// "Presets…": open the creator on a fresh scan of the presets folder.
    pub fn open_presets(&mut self) {
        self.presets_model().rescan();
        self.presets_open = true;
        if !self.presets_popped {
            self.centre_tab = CentreTab::Presets;
            self.reveal_centre = true;
        }
    }

    /// Open the TruePBR JSON editor: in front of the centre, or in its own window when popped out (Note FB7).
    pub fn open_json_editor(&mut self) {
        self.json_editor_open = true;
        if !self.json_editor_popped {
            self.centre_tab = CentreTab::Json;
            self.reveal_centre = true;
        }
    }

    /// The view in front of the centre column: `centre_tab` while that view is open and docked, else the selection.
    pub fn centre_front(&self) -> CentreTab {
        match self.centre_tab {
            CentreTab::Presets if self.presets_open && !self.presets_popped => CentreTab::Presets,
            CentreTab::Json if self.json_editor_open && !self.json_editor_popped => CentreTab::Json,
            _ => CentreTab::Selection,
        }
    }

    /// The header's Preset menu (AM12): every selectable preset whose chain targets this target, or none.
    pub fn preset_choices(&mut self) -> Vec<String> {
        let t = self.target;
        let pm = self.presets_model();
        let r = pm.registry();
        pm.list()
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| {
                r.chain(id)
                    .ok()
                    .and_then(|c| c.iter().rev().find_map(|p| p.target))
                    .is_none_or(|pt| pt == t)
            })
            .collect()
    }

    /// `preset` in `sdc.toml`, if it names one.
    pub fn project_preset(&self) -> Option<String> {
        self.project.as_ref()?.preset()
    }

    /// AM14: `preset` in `sdc.toml` when it names neither a built-in nor a preset of the presets folder (deleted or
    /// renamed since). Reads the folder only for a non-built-in id.
    pub fn missing_preset(&self) -> Option<String> {
        let id = self.project_preset()?;
        let mut r = engine::presets::PresetRegistry::builtin().ok()?;
        if r.get(&id).is_some() {
            return None;
        }
        if let Some(d) = &self.user_presets {
            r.add_dir(d);
        }
        r.get(&id).is_none().then_some(id)
    }

    /// Note FC1 (AM14): the project's `preset` when it names no installed preset, as the last plan request found it.
    pub fn missing_preset_id(&self) -> Option<&str> {
        self.missing_preset_id.as_deref()
    }

    /// Pick the project's preset (`None`: the target's default), one undo step, re-planned.
    pub fn set_preset(&mut self, id: Option<String>) {
        self.edit("Preset", move |p| p.set_preset(id.as_deref()));
    }

    pub fn set_target(&mut self, t: Target) {
        self.edit("Mode", move |p| p.set_target(t));
    }

    /// Remove an unmatched `[files]` entry (one that names no file under the current target) and plan again.
    pub fn remove_unmatched(&mut self, key: &str) {
        let k = key.to_string();
        self.edit("Remove entry", move |p| p.remove_file_entry(&k));
    }

    /// The BC1 decoder for the next launch. The session's model is fixed, so a difference needs a restart.
    /// Probe NVTT again, on a worker (DECISIONS AF3: a wrong program can take up to the probe timeout); the result
    /// lands in `nvtt_landed`. A newer probe supersedes an older one still running.
    pub fn refresh_nvtt(&mut self) {
        self.nvtt_gen += 1;
        self.nvtt_pending = true;
        self.nvtt_key = Some(self.nvtt_file());
        let (generation, path) = (self.nvtt_gen, self.settings.nvtt_path.clone());
        spawn_guarded(self.tx.clone(), "nvtt-probe", move |tx| {
            // A panic is this generation's result, so a stale probe's panic never ends the current one's wait.
            let probe = std::panic::catch_unwind(|| engine::nvtt::probe(path.as_deref()))
                .unwrap_or_else(|p| {
                    engine::nvtt::NvttProbe::Missing(format!(
                        "the probe panicked: {}",
                        crate::worker::panic_text(&*p)
                    ))
                });
            let _ = tx.send(WorkerMsg::Nvtt { generation, probe });
        });
    }

    /// Window focus (DECISIONS AF3): probe again only when the file NVTT resolves to changed (path, size or
    /// modified time) since the last probe, so a wrong program that takes focus is not started over and over.
    pub fn refresh_nvtt_if_changed(&mut self) {
        if self.nvtt_key.as_ref() != Some(&self.nvtt_file()) {
            self.refresh_nvtt();
        }
    }

    /// The file a probe now would run: the configured or located path, with its size and modified time.
    fn nvtt_file(&self) -> Option<NvttKey> {
        let (p, _) = engine::nvtt::locate(
            self.settings.nvtt_path.as_deref(),
            std::env::var_os("PATH").as_deref(),
            std::path::Path::new(engine::nvtt::NVTT_DEFAULT),
        )
        .ok()?;
        let md = std::fs::metadata(&p).ok()?;
        Some((p, md.len(), md.modified().ok()))
    }

    fn nvtt_landed(&mut self, generation: u64, probe: engine::nvtt::NvttProbe) {
        if generation != self.nvtt_gen {
            return;
        }
        self.nvtt_pending = false;
        let now = Some(probe);
        // A change since the last probe (NVTT installed or removed meanwhile) re-plans, so textures on NVTT see it.
        let changed = self.nvtt.is_some() && self.nvtt != now;
        self.nvtt = now;
        if changed {
            self.replan();
        }
    }

    pub fn nvtt_found(&self) -> bool {
        matches!(self.nvtt, Some(engine::nvtt::NvttProbe::Found(_)))
    }

    /// The Advanced drawer's NVTT status line.
    pub fn nvtt_status(&self) -> String {
        use engine::nvtt::NvttProbe;
        match &self.nvtt {
            Some(NvttProbe::Found(t)) if t.tested() => {
                format!("NVTT {} at {} (tested)", t.version, t.path.display())
            }
            Some(NvttProbe::Found(t)) => format!(
                "NVTT {} at {}: untested (3.2.5 is), acknowledge its version warning to use it",
                t.version,
                t.path.display()
            ),
            Some(NvttProbe::Missing(why)) => format!("NVTT not found ({why})"),
            None => "NVTT not checked yet".into(),
        }
    }

    /// A new NVTT path (or `None`: automatic). Saved, probed, and re-planned so the textures on NVTT see it.
    pub fn set_nvtt_path(&mut self, p: Option<PathBuf>) {
        self.settings.nvtt_path = p;
        self.save_settings();
        self.refresh_nvtt();
        self.replan();
    }

    pub fn set_reduce_motion(&mut self, on: bool) {
        self.settings.reduce_motion = on;
        self.save_settings();
    }

    pub fn set_bc1_choice(&mut self, c: crate::settings::Bc1Choice) {
        self.settings.bc1_decoder = c;
        self.save_settings();
        self.restart_needed = self
            .session
            .as_ref()
            .is_some_and(|s| s.bc1_model() != c.model());
    }

    /// The BC1 decoder the session runs with. The setting may differ (it applies after a restart), so anything that
    /// must match what the engine did here (the CLI command) reads this.
    pub fn session_bc1(&self) -> crate::settings::Bc1Choice {
        self.session
            .as_ref()
            .map_or(self.settings.bc1_decoder, |s| {
                crate::settings::Bc1Choice::from_model(s.bc1_model())
            })
    }

    pub fn is_dirty(&self) -> bool {
        self.project.as_ref().is_some_and(ProjectDoc::is_dirty)
            || self.tentative_edits().is_some()
            || self
                .sets_doc
                .as_ref()
                .is_some_and(crate::sets_doc::SetsDoc::is_dirty)
    }

    /// Note FI4 (owner O1 (b)): the only unsaved change is FC1's automatic recording, still the top step. It neither
    /// asks on close, open or reload nor marks the title; Save stays live and writes it.
    /// shortcut: a user edit followed by its Undo leaves `pushes` advanced, so that state prompts although the text
    /// equals the automatic state; compare the document with the recording's snapshot if users report it.
    pub fn only_automatic_unsaved(&self) -> bool {
        self.auto_note
            .as_ref()
            .is_some_and(|a| a.clean_before && a.step == self.history.pushes)
            && !self
                .sets_doc
                .as_ref()
                .is_some_and(crate::sets_doc::SetsDoc::is_dirty)
            && self.tentative_edits().is_none()
    }

    /// Unsaved changes the close, open and reload dialogs ask about: a user's, not only automatic ones (Note FI4).
    pub fn prompts_unsaved(&self) -> bool {
        self.is_dirty() && !self.only_automatic_unsaved()
    }

    /// The unsaved `sdc.toml` edits stashed by a blame that is still tentative (the retry plan has not landed), so
    /// prompts and recovery still see them (review M1 of #9).
    pub(crate) fn tentative_edits(&self) -> Option<String> {
        self.project
            .as_ref()
            .filter(|_| self.ignored_by_plan)
            .and_then(ProjectDoc::stashed_edits)
    }

    /// The unsaved documents as `(file, text)`, for the panic hook's recovery files (Note AR).
    pub fn recovery_snapshot(&self) -> Vec<(std::path::PathBuf, String)> {
        let mut out = vec![];
        // Note FI4: automatic types alone are recorded again on the next plan, so a restore never offers them.
        let only_auto = self.only_automatic_unsaved();
        if let Some(p) = self.project.as_ref().filter(|p| p.is_dirty() && !only_auto) {
            out.push((p.path().to_path_buf(), p.text()));
        }
        if let (Some(p), Some(text)) = (self.project.as_ref(), self.tentative_edits()) {
            out.push((p.path().to_path_buf(), text));
        }
        if let Some(s) = self
            .sets_doc
            .as_ref()
            .filter(|s| s.is_dirty() && !s.read_only())
        {
            out.push((s.path().to_path_buf(), s.text()));
        }
        out
    }

    /// [`Self::recovery_snapshot`] when a document changed since the last call, else `None` (audit item 11: the
    /// app refreshes the panic hook's copy only then).
    pub fn recovery_changed(&mut self) -> Option<Vec<(std::path::PathBuf, String)>> {
        let key = (
            self.project
                .as_ref()
                .map(|p| (p.rev(), p.is_dirty(), self.only_automatic_unsaved())),
            self.sets_doc
                .as_ref()
                .map(|s| (s.rev(), s.is_dirty(), s.read_only())),
        );
        if self.recovery_key == Some(key) {
            return None;
        }
        self.recovery_key = Some(key);
        Some(self.recovery_snapshot())
    }

    /// Raise the dialog for a failed save of `kind`; the error text names the file.
    fn save_failed(&mut self, kind: DocKind, e: &SaveError) {
        self.dialog = Some(match e {
            SaveError::ChangedOnDisk => Dialog::ExternalChange(kind),
            // Note FC10 (item 27): a Problems row with `Clear read-only flag`, not a dead-end dialog.
            SaveError::ReadOnlyFlag => {
                self.refresh_read_only_flags();
                self.status_line = format!("{} is read-only: not saved", kind.file());
                self.open_problems(None);
                return;
            }
            e => Dialog::Error(format!("{}: {e}", kind.file())),
        });
    }

    /// Note FC10 (item 27): which project files have the read-only flag; a change rebuilds Problems. Stats two files,
    /// so it runs with the banner (open, reload, save), never per frame (ui-rules 9.4).
    fn refresh_read_only_flags(&mut self) {
        let now: Vec<PathBuf> = [
            self.project.as_ref().map(|p| p.path()),
            self.sets_doc.as_ref().map(|s| s.path()),
        ]
        .into_iter()
        .flatten()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.permissions().readonly()))
        .map(|p| p.to_path_buf())
        .collect();
        if now != self.read_only_flags {
            self.read_only_flags = now;
            self.invalidate_views();
        }
    }

    /// Note FC10 (item 27): `Clear read-only flag`. Only this project's `sdc.toml` or `sdc-project.toml`; then the
    /// documents are reloaded when nothing is unsaved (unsaved edits are kept for the Save the flag refused).
    pub fn clear_read_only_flag(&mut self, path: &std::path::Path) -> Result<(), String> {
        let ours = [
            self.project.as_ref().map(|p| p.path()),
            self.sets_doc.as_ref().map(|s| s.path()),
        ]
        .into_iter()
        .flatten()
        .any(|p| p == path);
        if !ours {
            return Err(format!("{} is not this project's file", path.display()));
        }
        let fail = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut perm = std::fs::metadata(path).map_err(fail)?.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(path, perm).map_err(fail)?;
        if self.prompts_unsaved() {
            self.status_line = "Read-only flag cleared: Save to write your changes".into();
            self.refresh_banner();
        } else {
            self.reload_documents();
        }
        Ok(())
    }

    /// Note FC10 (T8): `Remove aliases from {id}`. Drops the `suffix_aliases` table of the user preset file `file`
    /// (comments and order kept), then plans again so the preset loads. Only a file in the user presets folder.
    pub fn remove_preset_aliases(&mut self, file: &std::path::Path) -> Result<(), String> {
        let inside = self
            .user_presets
            .as_deref()
            .is_some_and(|d| file.parent() == Some(d));
        if !inside {
            return Err(format!(
                "{} is not in the user presets folder",
                file.display()
            ));
        }
        if self.export.running.is_some() {
            return Err(also::EXPORT_RUNNING.into());
        }
        let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", file.display());
        let text = std::fs::read_to_string(file).map_err(|e| fail(&e))?;
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| fail(&e))?;
        doc.remove("suffix_aliases");
        engine::dds::write_atomic(file, doc.to_string().as_bytes()).map_err(|e| fail(&e))?;
        self.replan();
        Ok(())
    }

    /// Show `text` as a banner that survives every rebuild until [`AppModel::dismiss_kept_banner`].
    pub fn keep_banner(&mut self, text: String) {
        self.kept_banner = Some(text);
        self.refresh_banner();
    }

    pub fn dismiss_kept_banner(&mut self) {
        self.kept_banner = None;
        self.refresh_banner();
    }

    /// Rebuild the banner from both files: `sdc.toml`'s read-only reason and `sdc-project.toml`'s (P8).
    pub fn refresh_banner(&mut self) {
        self.refresh_read_only_flags();
        let mut lines = Vec::new();
        lines.extend(self.kept_banner.clone());
        // Note FC1 (AM14): a missing preset is a Problems row that blocks the export, not a banner.
        // Note FB5: a crash's recovery file is restored on open and needs no banner, except beside a document that
        // is read-only (it cannot take the edits, so the file stays and the banner names it).
        if let Some(p) = self.project.as_ref().filter(|p| p.read_only()) {
            if let Some(e) = p.error() {
                lines.push(read_only_banner(e));
            }
            let held = self.held_recovery.contains(p.path());
            lines.extend(crate::crash_log::recovery_note(p.path(), &p.text(), held));
        }
        if let Some(s) = self.sets_doc.as_ref().filter(|s| s.read_only()) {
            if let Some(e) = s.error() {
                lines.push(format!(
                    "{e} — sets and PBR JSON settings are read-only until it is fixed and reloaded"
                ));
            }
            let held = self.held_recovery.contains(s.path());
            lines.extend(crate::crash_log::recovery_note(s.path(), &s.text(), held));
        }
        // Note FB8: a line that names a code goes to the log as the engine wrote it, once per appearance; the banner
        // itself carries the words only.
        for l in lines.iter().filter(|l| !self.banner_logged.contains(l)) {
            if let Some(c) = code_text::first_code(l) {
                use engine::codes::Severity as S;
                let severity = match c.severity() {
                    S::Info => messages::Severity::Info,
                    S::Warning | S::Blocking => messages::Severity::Warning,
                    S::Error => messages::Severity::Error,
                };
                self.messages.log(severity, l.clone());
            }
        }
        self.banner = if lines.is_empty() {
            None
        } else {
            let shown: Vec<_> = lines.iter().map(|l| code_text::banner_line(l)).collect();
            Some(shown.join("\n"))
        };
        self.banner_logged = lines;
    }

    /// Reload both documents from disk (the banner's Reload), then plan once.
    /// The banner's Reload: at once when nothing is unsaved, else ask first (`Dialog::ReloadUnsaved`).
    pub fn request_reload(&mut self) {
        if self.prompts_unsaved() {
            self.dialog = Some(Dialog::ReloadUnsaved);
        } else {
            self.reload_documents();
        }
    }

    pub fn reload_documents(&mut self) {
        self.dialog = None;
        // K4, K5: Review and an export queued behind a plan belong to the documents being dropped.
        self.close_review();
        // Review Focus 2: so does a JSON answer; the running export asks instead (the safe side).
        self.pending_json_answer = None;
        self.export_after_plan = false;
        self.continue_after_plan = None;
        self.reset_type_follow();
        if let Some(p) = self.project.as_mut() {
            p.reload();
        }
        if let Some(s) = self.sets_doc.as_mut() {
            s.reload();
        }
        self.ignored_by_plan = false;
        self.drop_undo_note();
        self.history.clear();
        self.apply_recovery();
        self.refresh_banner();
        self.replan();
    }

    /// Save `sdc.toml`, then `sdc-project.toml`; the first error is returned (and raises its dialog). A clean
    /// document is not written (audit 2026-10-14 finding 3: writing it would offer "Overwrite" over a hand edit
    /// the GUI never loaded).
    pub fn save(&mut self) -> Result<(), SaveError> {
        let mut wrote = false;
        if let Some(p) = self.project.as_mut().filter(|p| p.is_dirty()) {
            if let Err(e) = p.save(false) {
                self.save_failed(DocKind::SdcToml, &e);
                return Err(e);
            }
            // Wave 2 #3: only a user's save holds the recovered edits, so only it drops the `.recovery` file.
            crate::crash_log::discard_recovery(p.path());
            wrote = true;
        }
        // A clean sdc-project.toml is not written (no empty file, no spurious conflict); a read-only one never has edits.
        if let Some(s) = self
            .sets_doc
            .as_mut()
            .filter(|s| s.is_dirty() && !s.read_only())
        {
            if let Err(e) = s.save(false) {
                self.save_failed(DocKind::Project, &e);
                return Err(e);
            }
            crate::crash_log::discard_recovery(s.path());
            wrote = true;
        }
        // The deferred provenance is written now (M2).
        self.drop_provenance_note();
        if wrote {
            self.refresh_banner(); // the save removed the saved documents' recovery files
            self.messages.notify(messages::Severity::Info, "Saved");
        }
        Ok(())
    }

    pub fn resolve_external_change(&mut self, which: DocKind, overwrite: bool) {
        self.dialog = None;
        match which {
            DocKind::SdcToml => {
                let Some(p) = self.project.as_mut() else {
                    return;
                };
                if overwrite {
                    match p.save(true) {
                        Ok(()) => crate::crash_log::discard_recovery(p.path()),
                        Err(e) => self.save_failed(which, &e),
                    }
                } else {
                    p.reload();
                    self.drop_undo_note();
                    self.history.clear();
                    self.ignored_by_plan = false;
                    self.refresh_banner();
                    self.replan();
                }
            }
            DocKind::Project => {
                let Some(s) = self.sets_doc.as_mut() else {
                    return;
                };
                if overwrite {
                    match s.save(true) {
                        Ok(()) => crate::crash_log::discard_recovery(s.path()),
                        Err(e) => self.save_failed(which, &e),
                    }
                } else {
                    s.reload();
                    self.drop_undo_note();
                    self.history.clear();
                    self.refresh_banner();
                    self.replan();
                }
            }
        }
    }

    /// Open `root`, or ask first when unsaved edits would be lost. True when the folder was opened.
    pub fn request_open(&mut self, root: PathBuf) -> bool {
        if self.export.running.is_some() {
            self.dialog = Some(Dialog::ExportRunning(AfterExport::Open(root)));
            false
        } else if self.gen_writing() {
            self.defer_until_generated(AfterExport::Open(root));
            false
        } else if self.prompts_unsaved() {
            self.dialog = Some(Dialog::OpenUnsaved(root));
            false
        } else {
            self.open_folder(root);
            true
        }
    }

    /// The folder a dropped path stands for: a folder is itself, a file is the folder holding it (M3a spec §5.1).
    pub fn drop_folder(path: &std::path::Path) -> Option<PathBuf> {
        if path.is_dir() {
            Some(path.to_path_buf())
        } else if path.is_file() {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(PathBuf::from)
        } else {
            None
        }
    }

    /// Files or folders dropped on the window: the first one that names a folder is opened through `request_open`,
    /// so unsaved edits and a running export still ask. True when a folder was opened.
    pub fn dropped(&mut self, paths: &[PathBuf]) -> bool {
        match paths.iter().find_map(|p| Self::drop_folder(p)) {
            Some(root) => self.request_open(root),
            None => false,
        }
    }

    /// The answer to `Dialog::OpenUnsaved`: save (opening only if that succeeded) or drop the edits, then open.
    pub fn resolve_open_unsaved(&mut self, root: PathBuf, save: bool) {
        self.dialog = None;
        // Audit 08f-1: an export started while the question was up; opening now would drop its JSON. Ask to stop
        // it first, as Open does; the question comes back once it has ended.
        if self.export.running.is_some() {
            self.dialog = Some(Dialog::ExportRunning(AfterExport::Open(root)));
            return;
        }
        if save {
            if self.save().is_err() {
                return;
            }
        } else {
            if let Some(p) = self.project.as_mut() {
                p.reload();
                self.drop_undo_note();
                self.history.clear();
            }
            if let Some(s) = self.sets_doc.as_mut() {
                s.reload();
                self.drop_undo_note();
                self.history.clear();
            }
        }
        self.open_folder(root);
    }

    pub fn request_close(&mut self) -> bool {
        if self.export.running.is_some() {
            self.dialog = Some(Dialog::ExportRunning(AfterExport::Close));
            false
        } else if self.gen_writing() {
            self.defer_until_generated(AfterExport::Close);
            false
        } else if self.prompts_unsaved() {
            self.dialog = Some(Dialog::CloseUnsaved);
            false
        } else {
            true
        }
    }
}

/// Delete a file a command created, if it still has the SHA-256 it was created with.
/// Redo of a Duplicate: copy the source to the copy's path again, never over a file already there; the new
/// copy's hash.
fn copy_again(cf: &history::CreatedFile) -> Result<String, String> {
    let fail = |e: std::io::Error| format!("could not copy {} again: {e}", cf.source.display());
    let mut out = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&cf.path)
    {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!(
                "{} already exists, so redo does not copy over it. Move or delete it, then redo.",
                cf.path.display()
            ))
        }
        Err(e) => return Err(fail(e)),
        Ok(f) => f,
    };
    // From here the file at `cf.path` is the one just created, so a failure removes it.
    let copied =
        std::fs::File::open(&cf.source).and_then(|mut src| std::io::copy(&mut src, &mut out));
    drop(out);
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&cf.path);
        return Err(fail(e));
    }
    history::sha256_of(&cf.path).ok_or_else(|| {
        let _ = std::fs::remove_file(&cf.path);
        format!("could not read the copy {}", cf.path.display())
    })
}

fn remove_created(cf: &history::CreatedFile) -> Result<(), String> {
    match history::sha256_of(&cf.path) {
        None => Ok(()), // already gone
        Some(h) if h == cf.sha256 => std::fs::remove_file(&cf.path)
            .map_err(|e| format!("could not delete {}: {e}", cf.path.display())),
        Some(_) => Err(format!(
            "{} has changed since it was duplicated, so undo keeps it. Delete it yourself if you no longer need it.",
            cf.path.display()
        )),
    }
}

/// The toast of a finished export: "Export finished: 12 written, 3 skipped, 0 failed".
pub fn export_finished_text(s: &engine::api::BatchSummary) -> String {
    format!(
        "Export finished: {} written, {} skipped, {} failed",
        s.converted, s.skipped, s.failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn bare_model() -> AppModel {
        let session =
            Arc::new(Session::new(encoders_sys::Bc1Model::Ideal).expect("an encoder session"));
        AppModel::new(GuiSettings::default(), None, session)
    }

    /// Owner answer 8: an idle model has no v2 job in flight, and `wait_idle` returns at once.
    #[test]
    fn an_idle_model_has_no_v2_jobs_pending() {
        let mut m = bare_model();
        assert!(!m.v2_jobs_pending());
        assert!(m.wait_idle(Duration::from_secs(1)));
    }

    pub(super) fn panicked(what: &'static str) -> WorkerMsg {
        WorkerMsg::Panicked {
            what,
            message: "boom".into(),
        }
    }

    /// Audit GM-10: mesh evidence that differs on every read never settles; `auto_replans` stops the re-plans
    /// after two rounds, and Export then refuses with "the sets changed while planning".
    #[test]
    fn alternating_mesh_votes_stop_replanning_after_two_rounds() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("textures/landscape");
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::from_pixel(16, 16, image::Rgb([90, 90, 90]))
            .save(root.join("rock01.png"))
            .unwrap();
        // Any file: each plan's evidence read then comes as a worker message, which this test replaces.
        let db = d.path().join("scan.db");
        std::fs::write(&db, "not a database").unwrap();
        let settings = GuiSettings {
            export_folder: Some(d.path().join("export")),
            ..GuiSettings::default()
        };
        let session =
            Arc::new(Session::new(encoders_sys::Bc1Model::Ideal).expect("an encoder session"));
        let mut m = AppModel::new(settings, None, session);
        // Note FC1: no High accept, so no recording adds its own plan to the count.
        m.accept = AcceptSuggestions::None;
        m.scan_db = Some(db);
        let start = m.generation;
        m.open_folder(root);
        let (mut flip, mut reads) = (false, 0);
        let end = Instant::now() + Duration::from_secs(120);
        while m.busy() {
            assert!(Instant::now() < end, "the plan never settled");
            let Ok(msg) = m.rx.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let msg = match msg {
                WorkerMsg::MeshVotes { generation, .. } => {
                    flip = !flip;
                    reads += 1;
                    let gras = [if flip { "a" } else { "b" }.to_string()].into();
                    WorkerMsg::MeshVotes {
                        generation,
                        result: Ok(MeshEvidence {
                            gras,
                            ..Default::default()
                        }),
                    }
                }
                other => other,
            };
            m.handle(msg);
        }
        assert_eq!(reads, 3, "one read per plan");
        assert_eq!(m.generation - start, 3, "the first plan and two re-plans");
        m.export_clicked();
        assert!(
            matches!(&m.dialog, Some(Dialog::Error(e)) if e.contains("sets changed while planning")),
            "{:?}",
            m.dialog
        );
    }

    /// Trace-debug 2026-10-22 #4 (Note CW1): a scan that starts while a mesh-evidence read is in flight must not let
    /// that read land the emptied store's votes, alpha and GRAS.
    #[test]
    fn a_scan_start_drops_the_evidence_read_in_flight() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("scan.db");
        std::fs::write(&db, "not a database").unwrap();
        let mut m = bare_model();
        m.scan_db = Some(db);
        m.gras_diffuses = ["keep".to_string()].into();
        m.refresh_mesh_votes(false);
        let read = m.votes_gen;
        assert!(m.votes_pending.is_some(), "a read is in flight");
        m.scan_started();
        // The read ends after `begin_scan`: the store reads empty.
        m.handle(WorkerMsg::MeshVotes {
            generation: read,
            result: Ok(Default::default()),
        });
        assert_eq!(
            m.gras_diffuses,
            ["keep".to_string()].into(),
            "the in-flight read landed empty evidence"
        );
        assert!(m.votes_pending.is_none(), "the pending read still settles");
    }

    /// A TruePBR rock set exported once, its PGPatcher JSON edited by hand, a setting changed and a second export
    /// started: the returned model is mid-export; `run_export` drives it.
    fn hand_edited_json_and_a_second_export(d: &std::path::Path) -> (AppModel, PathBuf) {
        let root = d.join("textures/landscape");
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 90])
        })
        .save(root.join("rock01.png"))
        .unwrap();
        image::RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 255]))
            .save(root.join("rock01_normal.png"))
            .unwrap();
        let settings = GuiSettings {
            export_folder: Some(d.join("export")),
            ..GuiSettings::default()
        };
        let session =
            Arc::new(Session::new(encoders_sys::Bc1Model::Ideal).expect("an encoder session"));
        let mut m = AppModel::new(settings, None, session);
        m.accept = AcceptSuggestions::None;
        m.open_folder(d.join("textures"));
        assert!(m.wait_idle(Duration::from_secs(60)));
        m.set_export_folder(d.join("export"));
        assert!(m.wait_idle(Duration::from_secs(60)));
        m.decide("landscape/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
        m.decide(
            "landscape/rock01_normal.png",
            Answer::Subtype(Subtype::PbrNormal),
        );
        m.decide(
            "landscape/rock01_normal.png",
            Answer::Green(GreenConvention::Directx),
        );
        assert!(m.wait_idle(Duration::from_secs(60)));
        let export = |m: &mut AppModel| {
            m.export_clicked();
            if m.review.is_some() {
                m.acknowledge_all_asks();
                m.export_from_review();
            }
        };
        export(&mut m);
        assert!(m.wait_idle(Duration::from_secs(120)));
        let p = m
            .export_root()
            .join("PBRNIFPatcher")
            .join(format!("{}.json", pbr_export::json_name(&m)));
        assert!(p.exists(), "{p:?}: {:?}", m.export.notes);
        std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
        m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.06)));
        assert!(m.wait_idle(Duration::from_secs(60)));
        // Note FC5: Review waits for the answer; it is given for the file as it is now.
        m.export_clicked();
        m.answer_review_json(true);
        m.acknowledge_all_asks();
        m.export_from_review();
        assert!(m.export.running.is_some(), "{:?}", m.status_line);
        (m, p)
    }

    /// Handle worker messages until the export has ended; `before_done` runs just before `ExportDone` is handled.
    fn run_export(m: &mut AppModel, before_done: impl FnOnce(&mut AppModel)) {
        let end = Instant::now() + Duration::from_secs(120);
        let mut before_done = Some(before_done);
        while m.export.running.is_some() {
            assert!(Instant::now() < end, "the export never ended");
            let Ok(msg) = m.rx.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            if matches!(msg, WorkerMsg::ExportDone(_)) {
                if let Some(f) = before_done.take() {
                    f(m);
                }
            }
            m.handle(msg);
        }
    }

    /// Note FC5 (supersedes N1, #44 and #48's overwrite question): a JSON edited again during the export has no
    /// answer for its bytes at the end: it is kept, with a note and a message, and nothing asks after the export.
    #[test]
    fn a_json_edited_during_the_export_is_kept_with_a_note_and_no_dialog() {
        let d = tempfile::tempdir().unwrap();
        let (mut m, p) = hand_edited_json_and_a_second_export(d.path());
        let again = p.clone();
        run_export(&mut m, move |_| {
            std::fs::write(&again, "{ \"edited\": 2 }\n").unwrap();
        });
        assert!(m.dialog.is_none(), "{:?}", m.dialog);
        assert!(std::fs::read_to_string(&p).unwrap().contains("edited"));
        let note = |t: &str| t.contains("PGPatcher JSON not written");
        assert!(
            m.export.notes.iter().any(|n| note(n)),
            "{:?}",
            m.export.notes
        );
        assert!(m.messages.entries().any(|e| note(&e.text)));
    }

    /// Note FC5: a cancelled run makes no NIF export due; a run that ended otherwise (here a failure) does.
    #[test]
    fn a_cancelled_run_makes_no_nif_export_due() {
        let mut m = bare_model();
        let c = CancelToken::default();
        c.cancel();
        m.export.running = Some(c);
        m.nif_export_queued = true;
        m.end_export_with(Err("cancelled".into()));
        assert!(!m.take_nif_export_due());
        m.export.running = Some(CancelToken::default());
        m.nif_export_queued = true;
        m.end_export_with(Err("boom".into()));
        assert!(m.take_nif_export_due());
        assert!(!m.take_nif_export_due());
        // A Review closed while its run still owes the NIF export keeps the NIF answers.
        m.nif_clear_answers_request = false;
        m.nif_export_queued = true;
        m.close_review();
        assert!(!m.nif_clear_answers_request);
        m.nif_export_queued = false;
        m.close_review();
        assert!(m.nif_clear_answers_request);
    }

    /// Audit 08f-9: an export-thread panic sets the error dialog once (`export_ended`'s), not twice.
    #[test]
    fn an_export_panic_is_handled_once() {
        let mut m = bare_model();
        m.export.running = Some(CancelToken::default());
        m.handle(WorkerMsg::Panicked {
            what: "export",
            message: "boom".into(),
        });
        assert_eq!(m.dialog, Some(Dialog::Error("internal error: boom".into())));
        assert_eq!(m.export.error.as_deref(), Some("internal error: boom"));
    }

    /// Review m1: an export-thread panic never replaces a dialog that is open (here the banner's Reload question).
    #[test]
    fn an_export_panic_keeps_an_open_dialog() {
        let mut m = bare_model();
        m.export.running = Some(CancelToken::default());
        m.dialog = Some(Dialog::ReloadUnsaved);
        m.handle(WorkerMsg::Panicked {
            what: "export",
            message: "boom".into(),
        });
        assert_eq!(m.dialog, Some(Dialog::ReloadUnsaved));
        assert_eq!(m.export.error.as_deref(), Some("internal error: boom"));
    }
}
