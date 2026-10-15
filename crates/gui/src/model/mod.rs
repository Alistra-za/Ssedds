//! The view-model (M3a spec §3): all GUI state and logic, no egui types, testable without a window.
pub mod auto_flags;
pub mod bulk;
pub mod dds_patch;
pub mod encoding_follow;
pub mod export;
pub mod fields;
pub mod heatmap;
pub mod history;
pub mod input_name;
pub mod inspector;
pub mod items;
pub mod known_paths;
pub mod meshes;
pub mod messages;
pub mod nav;
pub mod out_size;
pub mod pack_fills;
pub mod pbr_export;
mod pbr_follow;
pub mod picker;
pub mod pin;
pub mod plugin_refs;
pub mod presets;
pub mod preview;
pub mod readout;
pub mod rename;
pub mod rnm;
pub mod rnm_preview;
pub mod sections;
pub mod set_preview;
pub mod sets;
pub mod shell;
pub mod skin;
pub mod texture_view;
pub mod used_by;
pub mod view;
pub mod viewport3d;

use crate::project::{ProjectDoc, SaveError};
use crate::settings::GuiSettings;
use crate::worker::{
    spawn_export, spawn_guarded, spawn_plan, MeshEvidence, PlanFailure, WorkerMsg,
};
use engine::api::{BatchRequest, CancelToken};
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype, Target};
use engine::policy::{AcceptSuggestions, SourceStamp};
use engine::presets::ConfigFile;
use engine::session::Session;
use items::*;
use serde_json::json;
use std::path::PathBuf;

/// The file an NVTT probe ran on: path, size and modified time (`AppModel::refresh_nvtt_if_changed`).
type NvttKey = (PathBuf, u64, Option<std::time::SystemTime>);
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Item(String),
    Set(String),
}

pub use texture_view::Filter;

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
    /// The export asks about these undecided textures (item keys): decide them here, or proceed without.
    ProceedAnyway(Vec<String>),
    /// Export cannot start: these must be fixed first (M3b §6.3).
    ExportBlocked(Vec<pbr_export::ExportIssue>),
    /// Export can start after a "Proceed anyway" (M3b §6.3: matchN pitfalls).
    ExportWarnings(Vec<pbr_export::ExportIssue>),
    ExternalChange(DocKind),
    CloseUnsaved,
    /// Opening this folder would drop unsaved edits to the current `sdc.toml`.
    OpenUnsaved(PathBuf),
    /// An export is running; closing or opening would have to cancel it first (M3a spec §2.6).
    ExportRunning(AfterExport),
    /// The PGPatcher JSON was edited outside sdc since the last export; the new text waits for the answer.
    OverwriteJson(PathBuf),
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
    Error(String),
}

/// How the export continues once the plan that follows the undecided-texture dialog has landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Continue {
    /// Every row was decided: ask the undecided question again (a new plan can raise new ones), then export.
    Recheck,
    /// "Proceed anyway" on a plan that was not settled yet: export, skipping what is undecided.
    Start,
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
    mesh_votes: std::collections::BTreeMap<String, Vec<engine::shader::Shader>>,
    mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
    gras_diffuses: std::collections::BTreeSet<String>,
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
            mesh_votes: m.mesh_votes.clone(),
            mesh_alpha: m.mesh_alpha.clone(),
            gras_diffuses: m.gras_diffuses.clone(),
        }
    }
}

fn read_only_banner(error: &str) -> String {
    format!("{error} — sdc.toml is ignored and the GUI is read-only until it is fixed and reloaded")
}

/// The cached set views and export blocks, valid for `key` only.
#[derive(Default)]
pub(crate) struct ViewCache {
    pub key: Option<ViewKey>,
    pub views: Option<Arc<Vec<sets::SetView>>>,
    pub blocks: Option<Arc<Vec<pbr_export::ExportIssue>>>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ViewKey {
    items: u64,
    inputs: u64,
    doc: u64,
    root: Option<PathBuf>,
    export_root: PathBuf,
    target: Target,
}

pub struct AppModel {
    /// The file pickers (Task 6); one dialog at a time, shared with the sub-models.
    pub picker: picker::Picker,
    pub settings: GuiSettings,
    pub settings_path: Option<PathBuf>,
    /// `%APPDATA%\sdc\config.toml`, set by `main`; `None` in tests so a developer's own config cannot leak in.
    pub user_config: Option<PathBuf>,
    /// `%APPDATA%\sdc\presets`, set by `main`; `None` in tests so a developer's own presets cannot leak in.
    pub user_presets: Option<PathBuf>,
    /// `(id, file)` of the user presets the last plan loaded (the preset inspector lists them).
    pub loaded_user_presets: Vec<(String, String)>,
    /// `(file, reason)` of the user preset files the last plan skipped (CFG-PRESET-SKIPPED; the inspector lists them).
    pub skipped_user_presets: Vec<(String, String)>,
    /// The scan database the mesh votes read (`meshes::default_store_path()`), set by `main`; `None` in tests so a
    /// developer's scan cannot leak in.
    pub scan_db: Option<PathBuf>,
    /// S-shader-10: `shader::mesh_key` → what the scanned meshes report; sent as `BatchRequest::mesh_shaders`.
    pub mesh_votes: std::collections::BTreeMap<String, Vec<engine::shader::Shader>>,
    /// M6 Track E (DECISIONS Note U5): `shader::colour_mesh_key` → the alpha property of each scanned shape that uses
    /// the colour member; sent as `BatchRequest::mesh_alpha`.
    pub mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
    /// Scan evidence for the Subsurface auto mode (DECISIONS AG102): `shader::colour_mesh_key` → whether any scanned
    /// source shape that uses the colour member is Tree_Anim (SLSF2 bit 29). Not sent to the engine.
    pub mesh_tree_anim: std::collections::BTreeMap<String, bool>,
    /// Bumped whenever `mesh_tree_anim` or `known` is replaced (DECISIONS AG109); part of `json_key`.
    pub evidence_rev: u64,
    /// Complex grass design §5: the GRAS diffuses of the scan (`engine::grass::path_key` keys), read with the mesh
    /// evidence; sent as `BatchRequest::gras_diffuses`.
    pub gras_diffuses: std::collections::BTreeSet<String>,
    /// GUI tweaks G1/G5: the texture and mesh paths of the last scan, for the path pickers.
    pub known: Arc<known_paths::KnownPaths>,
    /// Item 9: the selected texture's "Used by meshes" read (`used_by_for`).
    pub used_by: used_by::UsedByState,
    /// G3: the TruePBR JSON editor window is open.
    pub json_editor_open: bool,
    /// Note AM: the preset creator window is open.
    pub presets_open: bool,
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
    /// Bumped whenever `items` is replaced (or announced with `invalidate_views`): the set-view cache key.
    items_rev: u64,
    /// Bumped whenever `last_inputs` is written: `engine_inputs` (so the export blocks) reads it.
    inputs_rev: u64,
    /// `TextureItem::key` to its index in `items` (audit 7).
    item_index: std::collections::HashMap<String, usize>,
    /// The cached set views and export blocks (DECISIONS Note Y7).
    pub(crate) views: std::cell::RefCell<ViewCache>,
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
    pub status_line: String,
    /// Item 20: the toasts and the Messages log (DECISIONS AG112-AG114); every status-line note also lands here.
    pub messages: messages::Messages,
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
    /// Task 9 (Z11): the last "encoding follows type" cleanup, as (status-line text, hover listing every texture).
    pub encoding_note: Option<(String, String)>,
    /// Per packed output key: fills a recipe switch removed because the new recipe has no such channel.
    pub dropped_fills: std::collections::BTreeMap<String, String>,
    pub dialog: Option<Dialog>,
    /// F3b D4: the rename dialog's last applied recipe, offered again on the next open (this session only).
    pub last_recipe: rename::Recipe,
    pub expanded: Option<String>,
    pub export: export::ExportModel,
    pub preview: preview::PreviewModel,
    /// The DDS header-patch choice (DECISIONS S7).
    pub dds_patch: dds_patch::DdsPatchState,
    pub view: view::ViewState,
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
    /// Keyboard navigation, the shortcut sheet and the command palette (items 5 and 19).
    pub nav: nav::Nav,
    /// Explorer requests the app has not started yet (Task 5, `model::shell`).
    pub shell: Vec<shell::ShellRequest>,
    /// File-exists answers asked while drawing (the scan DB, Show in Explorer), re-checked at most once a second
    /// (AG128).
    pub stats: std::cell::RefCell<shell::StatCache>,
    /// A Save report write is on a worker (AG130); the app keeps frames coming until it lands.
    pub report_saving: bool,
    /// Export was clicked while sources had changed: it starts when the fresh plan arrives.
    export_after_plan: bool,
    /// The undecided-texture dialog ended while a plan was in flight: the export continues when it lands.
    continue_after_plan: Option<Continue>,
    /// The user chose to cancel the running export and then close or open: done when the export has ended.
    after_export: Option<AfterExport>,
    /// What the running export writes afterwards (the JSON and the vanilla/CPM copies).
    pending_json: Option<pbr_export::PendingJson>,
    /// The JSON text waiting for the answer to `Dialog::OverwriteJson`.
    pending_text: Option<String>,
    /// `sdc.toml` was made read-only because the engine rejected it (not because it failed to parse).
    ignored_by_plan: bool,
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
    /// While a folder's first plan is in flight: the folder open before it (`None`: none) and Recent as it was.
    /// Cancel goes back to both.
    opening: Option<(Option<PathBuf>, Vec<PathBuf>)>,
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
            user_config: None,
            user_presets: None,
            loaded_user_presets: vec![],
            skipped_user_presets: vec![],
            scan_db: None,
            mesh_votes: Default::default(),
            mesh_alpha: Default::default(),
            mesh_tree_anim: Default::default(),
            evidence_rev: 0,
            gras_diffuses: Default::default(),
            known: Default::default(),
            used_by: Default::default(),
            json_editor_open: false,
            presets_open: false,
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
            inputs_rev: 0,
            item_index: Default::default(),
            views: Default::default(),
            sets: vec![],
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
            status_line: String::new(),
            messages: Default::default(),
            sizes: Default::default(),
            rnm_preview: Default::default(),
            status_note: None,
            undo_note: None,
            undo_note_at: None,
            dropped_fills: Default::default(),
            kept_banner: None,
            banner: None,
            encoding_note: None,
            dialog: None,
            last_recipe: Default::default(),
            expanded: None,
            export: export::ExportModel::default(),
            preview,
            dds_patch: Default::default(),
            view: view::ViewState::default(),
            gpu_max_dim: 16384,
            gpu_available: true,
            export_after_plan: false,
            continue_after_plan: None,
            after_export: None,
            pending_json: None,
            pending_text: None,
            ignored_by_plan: false,
            close_ready: false,
            restart_needed: false,
            nvtt: None,
            nvtt_gen: 0,
            nvtt_pending: false,
            nvtt_key: None,
            plugin_refs: Default::default(),
            nav: Default::default(),
            shell: Vec::new(),
            stats: Default::default(),
            report_saving: false,
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

    /// Write the patched copy of DDS item `key` to `<export root>/<job key>`. Every outcome lands in
    /// `dds_patch.last` (the panel shows it) and ends the confirmation.
    pub fn patch_dds_header(&mut self, key: &str) -> Result<PathBuf, String> {
        self.dds_patch.confirming = None;
        let r = match self.item(key).filter(|i| i.kind == ItemKind::Dds) {
            None => Err("not a DDS item".to_string()),
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
        self.dds_patch.last = Some((key.to_string(), line));
        r.map(|(_, dest)| dest)
    }

    pub fn open_folder(&mut self, root: PathBuf) {
        self.open_cancelled = false;
        // Whatever was pending for the previous folder no longer applies (spec §8).
        self.after_export = None;
        self.pending_json = None;
        self.pending_text = None;
        // Every dialog that can be open here belongs to the previous folder (`OpenUnsaved` and `ExportRunning`
        // were answered to get here). Audit 2026-10-14 finding 2: a `ProceedAnyway` left open would be closed by
        // the new folder's plan (`refresh_undecided_dialog`) and continue into an export of that folder.
        self.dialog = None;
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
        self.dropped_fills.clear();
        let same_folder = self.root.as_ref() == Some(&root);
        self.ignored_by_plan = false;
        let project = ProjectDoc::open(&root);
        self.project = Some(project);
        self.sets_doc = Some(crate::sets_doc::SetsDoc::open(&root));
        self.drop_undo_note();
        self.history.clear();
        self.refresh_banner();
        let recent = self.settings.recent.clone();
        self.settings.push_recent(root.clone());
        self.save_settings();
        let previous = self.root.replace(root);
        // A second open while the first is still planning goes back to the folder before both.
        if !(self.opening.is_some() && self.planning.is_some()) {
            self.opening = Some((previous, recent));
        }
        self.items.clear();
        self.invalidate_views();
        self.sets.clear();
        self.unmatched_keys.clear();
        self.selection = Selection::None;
        self.multi = Default::default();
        self.bulk = Default::default();
        self.expanded = None;
        self.preview_close();
        // The drawer shows the folder it ran on; another folder starts with an empty one.
        if !same_folder && self.export.running.is_none() {
            self.export = export::ExportModel::default();
        }
        self.replan();
    }

    pub fn save_settings(&self) {
        if let Some(p) = &self.settings_path {
            let _ = self.settings.save(p);
        }
    }

    /// `<export folder>` from the settings, else `<root>/sdc-out`.
    pub fn export_root(&self) -> PathBuf {
        self.settings
            .export_folder
            .clone()
            .unwrap_or_else(|| self.root.clone().unwrap_or_default().join("sdc-out"))
    }

    /// What the next plan uses: the folder, recursive, High suggestions accepted (Note U3), the in-memory `sdc.toml` (DECISIONS N4).
    pub fn request(&self) -> Option<BatchRequest> {
        let root = self.root.clone()?;
        let project = self.project.as_ref()?;
        let mut req = BatchRequest::new(vec![root], self.export_root(), Target::Cs);
        req.target = None;
        req.recursive = true;
        req.accept = self.accept;
        req.project_config_inline = Some(if project.read_only() {
            ConfigFile::default()
        } else {
            let mut c = project.config().unwrap_or_default();
            // AM14: a preset that no longer exists is left out, so the plan runs on the fallback (the banner says so).
            if self.missing_preset().is_some() {
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
        req.mesh_shaders = self.mesh_votes.clone();
        req.mesh_alpha = self.mesh_alpha.clone();
        req.gras_diffuses = self.gras_diffuses.clone();
        Some(req)
    }

    /// S-shader-10, M6 Track E: read the mesh votes, the mesh alpha (keys from the colour-member jobs only, by
    /// `shader::colour_mesh_key`, the engine's rule) and the GRAS diffuses (complex grass design §5 condition 1) from
    /// the scan database on one worker thread, tagged with a generation; `votes_landed` applies the newest result.
    /// No database means no evidence (applied at once): the suffix rules decide and the alpha is asked. A read
    /// error keeps the previous values (stderr). `after_plan`: the result continues `apply_plan`'s tail.
    pub fn refresh_mesh_votes(&mut self, after_plan: bool) {
        let keys: std::collections::BTreeSet<String> = self
            .items
            .iter()
            .filter_map(|i| i.job.as_ref())
            .filter(|j| j.pack.is_none())
            .filter_map(|j| {
                engine::shader::colour_mesh_key(&j.game_path, &j.output, Some(j.profile.subtype))
            })
            .collect();
        // A scan-finished refresh must not drop the continuation of a pending post-plan read.
        if !after_plan {
            // A finished scan (or another database): the "Used by meshes" list reads again.
            self.used_by_scan_changed();
        }
        let after_plan = after_plan || self.votes_pending.is_some_and(|(_, a)| a);
        self.votes_gen += 1;
        let generation = self.votes_gen;
        self.votes_pending = Some((generation, after_plan));
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
                })
            };
            // A panic is an error of this generation (kept values, stderr), not a dialog for a stale read.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read))
                .unwrap_or_else(|p| Err(format!("panic: {}", crate::worker::panic_text(&*p))));
            let _ = tx.send(WorkerMsg::MeshVotes { generation, result });
        });
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
                let changed = e.votes != self.mesh_votes
                    || e.alpha != self.mesh_alpha
                    || e.gras != self.gras_diffuses;
                (self.mesh_votes, self.mesh_alpha, self.gras_diffuses) = (e.votes, e.alpha, e.gras);
                self.set_tree_anim(e.tree_anim);
                changed
            }
            Err(e) => {
                eprintln!(
                    "tessalith: scan database read failed, keeping the previous mesh evidence: {e}"
                );
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
        } else if std::mem::take(&mut self.export_after_plan) {
            // DECISIONS AF4: an Export clicked during this scan-finished read starts now, never on a later plan.
            self.export_clicked();
        }
    }

    /// Plan again on a worker; a newer plan supersedes (cancels) an older one.
    pub fn replan(&mut self) {
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
            mesh_votes: req.mesh_shaders.clone(),
            mesh_alpha: req.mesh_alpha.clone(),
            gras_diffuses: req.gras_diffuses.clone(),
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
        let Some((previous, recent)) = self.opening.take() else {
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
            }
            None => {
                self.root = None;
                self.project = None;
                self.sets_doc = None;
                self.last_inputs = None;
                // An export queued behind the cancelled plan has no folder to run on (AG129).
                self.export_after_plan = false;
                self.continue_after_plan = None;
                self.refresh_banner();
                self.status_line.clear();
                self.open_cancelled = true;
            }
        }
    }

    /// The hover of the header's Cancel while a folder opens.
    pub fn cancel_open_hover(&self) -> &'static str {
        match &self.opening {
            Some((Some(_), _)) => {
                "Stops opening and re-opens the previous folder (selection and undo history are reset)"
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

    /// Drain worker messages. True when anything changed (the UI repaints).
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for d in self.picker.take() {
            changed = true;
            self.picked(d);
        }
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            self.handle(msg);
        }
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
                    Ok(view) => self.apply_plan(view),
                    Err(f) => self.plan_failed(f),
                }
            }
            WorkerMsg::Planned { .. } => {}
            WorkerMsg::MeshVotes { generation, result } => self.votes_landed(generation, result),
            WorkerMsg::UsedBy { generation, result } => self.used_by_landed(generation, result),
            WorkerMsg::KnownPaths(result) => self.known_paths_landed(result),
            WorkerMsg::Nvtt { generation, probe } => self.nvtt_landed(generation, probe),
            WorkerMsg::Panicked { what, message } => {
                match what {
                    "export" => {
                        self.export.running = None;
                        self.run_after_export();
                    }
                    "preview" => self.preview_panicked(&message),
                    "heatmap" => self.heatmap_panicked(),
                    "rnm-preview" => self.rnm_preview_panicked(),
                    "dims" => self.dims_panicked(),
                    "used-by" => self.used_by_panicked(&message),
                    "save-report" => self.report_saving = false,
                    _ => {}
                }
                self.dialog = Some(Dialog::Error(format!(
                    "internal error in {what}: {message}"
                )));
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
                self.history.clear();
                self.refresh_banner();
                self.ignored_by_plan = true;
                self.replan();
                return;
            }
            // The plan without `sdc.toml` failed too: the config is not the project's (the user config, say).
            // Put the project back as the file has it, and raise the error.
            if std::mem::take(&mut self.ignored_by_plan) {
                if let Some(p) = self.project.as_mut() {
                    p.reload();
                    self.drop_undo_note();
                    self.history.clear();
                    self.refresh_banner();
                }
            }
        }
        self.status_line = format!("Planning failed: {}", f.message);
        self.dialog = Some(Dialog::Error(f.message));
    }

    /// Messages the export and the preview handle.
    fn handle_more(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::ReportSaved { path, result } => self.report_saved(&path, result),
            WorkerMsg::Event(e) => self.export.on_event(e),
            WorkerMsg::ExportDone(result) => {
                self.export.running = None;
                if let Some(p) = self.export.pace.as_mut() {
                    p.stop(Instant::now());
                }
                let mut failure = None;
                match result {
                    Ok(summary) => {
                        let sev = if summary.failed > 0 {
                            messages::Severity::Warning
                        } else {
                            messages::Severity::Info
                        };
                        self.messages.notify(sev, export_finished_text(&summary));
                        self.export.summary = Some(summary);
                    }
                    Err(e) => {
                        self.export.error = Some(e.clone());
                        failure = Some(e);
                    }
                }
                self.finish_json();
                // A user who cancelled the export to leave does not need its error as a dialog.
                if !self.run_after_export() {
                    if let Some(e) = failure {
                        self.dialog = Some(Dialog::Error(e));
                    }
                }
            }
            WorkerMsg::Source { token, result } => self.on_source(token, result),
            WorkerMsg::Encoded { token, result } => self.on_encoded(token, result),
            WorkerMsg::Heatmap { id, image } => self.heatmap_landed(id, image),
            WorkerMsg::Dims(dims) => self.dims_landed(dims),
            WorkerMsg::RnmPreview { id, image } => self.rnm_preview_landed(id, image),
            _ => {}
        }
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
    /// export folder is unusable, an `export_blocks` entry. True (with the dialog showing why) when one fired.
    /// Shared by `export_checks` and the continuation from the undecided dialog, whose decisions can change them.
    fn export_stopped(&mut self) -> bool {
        let Some(root) = self.root.clone() else {
            return true;
        };
        if self.plan_unsettled() {
            return true;
        }
        if let Some(problem) = export::export_folder_problem(&root, &self.export_root()) {
            self.dialog = Some(Dialog::Error(problem));
            return true;
        }
        let blocks = pbr_export::export_blocks(self);
        if !blocks.is_empty() {
            self.dialog = Some(Dialog::ExportBlocked(blocks));
            return true;
        }
        false
    }

    /// The export folder and undecided-texture checks, then the export itself.
    fn export_checks(&mut self) {
        if self.export_stopped() {
            return;
        }
        let asks = pbr_export::export_asks(self);
        if !asks.is_empty() {
            self.dialog = Some(Dialog::ExportWarnings(asks));
            return;
        }
        self.undecided_check();
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

    /// The undecided-texture question, then the export itself.
    fn undecided_check(&mut self) {
        let keys = self.undecided_keys();
        if !keys.is_empty() {
            self.dialog = Some(Dialog::ProceedAnyway(keys));
            return;
        }
        self.start_export();
    }

    /// Keys of the textures that still need a decision, in list order.
    pub fn undecided_keys(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::NeedsDecision)
            .map(|i| i.key.clone())
            .collect()
    }

    /// Whether the texture asks for its sub-type, and the engine's suggestion for it (the one the confirm-subtype
    /// box preselects). `None`: the sub-type is not what it waits for (an alpha or green-channel question).
    pub fn subtype_question(&self, key: &str) -> Option<Option<Subtype>> {
        self.item(key)?.decisions.iter().find_map(|d| match d {
            Decision::Subtype { suggested } => Some(*suggested),
            _ => None,
        })
    }

    /// One row of the export dialog: decide `key` as `st`. The row leaves the list when the plan that follows shows
    /// the texture decided (it may ask something else next); when none are left the export continues.
    pub fn decide_undecided_row(&mut self, key: &str, st: Subtype) {
        self.decide(key, Answer::Subtype(st));
        self.refresh_if_settled();
    }

    /// A row's inline answer that is not a sub-type (alpha meaning, green convention, an acknowledgement). The row
    /// leaves the list when the plan that follows shows the texture decided.
    pub fn answer_undecided_row(&mut self, key: &str, answer: Answer) {
        self.decide(key, answer);
        self.refresh_if_settled();
    }

    /// "Accept all suggestions": decide every listed texture that has a suggestion as that suggestion, as one undo
    /// step. A texture without a suggestion stays listed and undecided; alpha, green and acknowledgements are
    /// never answered here.
    pub fn accept_all_suggestions(&mut self) {
        let Some(Dialog::ProceedAnyway(keys)) = self.dialog.clone() else {
            return;
        };
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
        self.refresh_if_settled();
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
        let before = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        self.batch = true;
        f(self);
        self.batch = false;
        let after = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        if after == before {
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

    /// No plan in flight, so the items on screen are current: update the dialog's rows now. Otherwise the plan
    /// that lands does it.
    fn refresh_if_settled(&mut self) {
        if self.planning.is_none() {
            self.refresh_undecided_dialog();
        }
    }

    /// A plan landed while the dialog is open: rows whose texture is decided (or gone) leave the list. With none
    /// left the dialog closes and the export goes on.
    fn refresh_undecided_dialog(&mut self) {
        let Some(Dialog::ProceedAnyway(keys)) = self.dialog.as_ref() else {
            return;
        };
        let live: Vec<String> = keys
            .iter()
            .filter(|k| {
                self.item(k)
                    .is_some_and(|i| i.status == ItemStatus::NeedsDecision)
            })
            .cloned()
            .collect();
        if live.is_empty() {
            self.dialog = None;
            self.continue_export(Continue::Recheck);
        } else if let Some(Dialog::ProceedAnyway(keys)) = self.dialog.as_mut() {
            *keys = live;
        }
    }

    /// Continue the export from the undecided-texture dialog. The decisions re-planned: a plan still in flight
    /// is waited for (never export on a stale plan), then `how` decides what comes next.
    fn continue_export(&mut self, how: Continue) {
        if self.planning.is_some() {
            self.continue_after_plan = Some(how);
        } else {
            self.run_continue(how);
        }
    }

    fn run_continue(&mut self, how: Continue) {
        // The decisions made in the dialog can change the blocks (a diffuse decided as PBR switches the set's
        // checks on), so the guards run again. The asks are not repeated: the user already answered them.
        if self.export_stopped() {
            return;
        }
        match how {
            Continue::Recheck => self.undecided_check(),
            Continue::Start => self.start_export(),
        }
    }

    /// The answer "Proceed anyway" to `ExportWarnings`: on to the undecided-texture question, then the export.
    pub fn proceed_with_warnings(&mut self) {
        self.dialog = None;
        self.undecided_check();
    }

    /// Mark every item whose source file differs from the stamp the plan took (or, for a DDS input, from the stamp
    /// taken when it was listed). A packed output is checked against each of its sources, and a raw source feeding
    /// one is flagged with it. Returns the number of items flagged.
    pub fn check_sources(&mut self) -> usize {
        // A hand edit of a clean `sdc.toml` is simply taken (nothing to lose): the next plan reads it. With unsaved
        // edits the save dialog still asks (audit 2026-10-14 finding 3).
        let hand_edited = self
            .project
            .as_ref()
            .is_some_and(|p| !p.is_dirty() && p.changed_on_disk());
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
        self.replan();
    }

    /// DECISIONS N3: undecided textures are skipped, never decided here.
    pub fn proceed_anyway(&mut self) {
        self.dialog = None;
        self.continue_export(Continue::Start);
    }

    /// Save first (the CLI command and the next session read `sdc.toml`), then run the export on a worker.
    fn start_export(&mut self) {
        if self.export.running.is_some() || self.save().is_err() {
            return;
        }
        let (Some(session), Some(mut req)) = (self.session.clone(), self.request()) else {
            return;
        };
        req.on_existing = engine::api::OnExisting::IfChanged;
        self.pending_json = pbr_export::prepare(self);
        self.pending_text = None;
        self.export = export::ExportModel {
            open: true,
            pace: Some(export::Pace::new(Instant::now())),
            ..Default::default()
        };
        let cancel = CancelToken::default();
        self.export.running = Some(cancel.clone());
        spawn_export(self.tx.clone(), session, req, cancel);
    }

    /// The export ended: write the copies and the JSON of the sets that exported (spec §6.1). Nothing is written
    /// after a cancelled or failed export.
    fn finish_json(&mut self) {
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
        let last = self.sets_doc.as_ref().and_then(|d| d.last_json_sha256());
        match pbr_export::finish(
            &p,
            &self.export.rows,
            last.as_deref(),
            &mut self.export.notes,
        ) {
            pbr_export::Finish::Written(sha) => self.record_json_sha(&sha),
            pbr_export::Finish::Overwrite(text) => {
                self.pending_text = Some(text);
                self.dialog = Some(Dialog::OverwriteJson(p.path.clone()));
                self.pending_json = Some(p);
            }
            pbr_export::Finish::Nothing => {}
        }
    }

    /// Record the hash of the JSON just written and save `sdc-project.toml`, so the record matches the disk. While
    /// the project file is read-only (a bad `sdc-project.toml`, ruling P8) the JSON is still written but no hash is
    /// recorded: the next export then asks before overwriting it.
    fn record_json_sha(&mut self, sha: &str) {
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        if d.read_only() {
            self.export
                .notes
                .push("the JSON's hash was not recorded: sdc-project.toml is read-only".into());
            return;
        }
        d.set_last_json_sha256(sha);
        if let Err(e) = d.save(false) {
            self.dialog = Some(Dialog::Error(format!("sdc-project.toml: {e}")));
        }
    }

    /// The answer to `Dialog::OverwriteJson`.
    pub fn resolve_overwrite_json(&mut self, overwrite: bool) {
        self.dialog = None;
        let (Some(p), Some(text)) = (self.pending_json.take(), self.pending_text.take()) else {
            return;
        };
        if overwrite {
            if let Some(sha) = pbr_export::overwrite(&p.path, &text, &mut self.export.notes) {
                self.record_json_sha(&sha);
            }
        } else {
            self.export.notes.push(format!(
                "PGPatcher JSON kept as edited: {}",
                p.path.display()
            ));
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
        self.invalidate_views();
        self.sets = view.sets;
        self.unmatched_keys = view.unmatched;
        self.rnm_notes = view.rnm_notes;
        self.rnm_keys = view.rnm_keys;
        self.batch_preset = view.preset_id;
        self.loaded_user_presets = view.user_presets;
        self.skipped_user_presets = view.skipped_user_presets;
        // The count of textures that need a decision is shown (amber) in the Textures column header.
        self.status_line = format!("{} textures, {} sets", self.items.len(), self.sets.len());
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

    /// The tail of `apply_plan`, once the scan-database evidence of this plan is in.
    fn finish_plan(&mut self) {
        let now = EngineInputs::of(self);
        if self.last_inputs.as_ref() != Some(&now) && self.auto_replans < 2 {
            self.auto_replans += 1;
            self.replan();
            return;
        }
        self.auto_replans = 0;
        // Task 9 (Z11): the final pass; a cleanup re-plans once.
        if self.settle_types() {
            return;
        }
        // Both flags are taken together so a leftover one cannot fire on a later, unrelated plan; the export the
        // user clicked wins.
        let (clicked, continued) = (
            std::mem::take(&mut self.export_after_plan),
            self.continue_after_plan.take(),
        );
        if clicked {
            self.export_checks();
        } else if let Some(how) = continued {
            self.run_continue(how);
        } else {
            self.refresh_undecided_dialog();
        }
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            self.poll();
            if !self.busy() {
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
            export_root: self.export_root(),
            target: self.target,
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

    /// Select one texture; any multi-selection ends (F3a D1).
    pub fn select_item(&mut self, key: &str) {
        self.selection = Selection::Item(key.to_string());
        self.multi = Default::default();
        self.bulk = Default::default();
    }

    /// Select one set; any multi-selection ends (F3a D1).
    pub fn select_set(&mut self, key: &str) {
        self.selection = Selection::Set(key.to_string());
        self.multi = Default::default();
        self.bulk = Default::default();
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
        let (Some(p), Some(s)) = (self.project.as_mut(), self.sets_doc.as_mut()) else {
            return;
        };
        // A batch records one step around all its writes (`batch_edit`): no snapshot per write.
        if self.batch {
            return f(p, s);
        }
        let before = Self::snapshot_of(Some(p), Some(s));
        f(p, s);
        let after = Self::snapshot_of(Some(p), Some(s));
        if after == before {
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
        // Task 9 (Z11): restoring a state as it was is no sub-type change to clean up after.
        self.reset_type_follow();
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

    /// Show "Undid: <label>" (or "Redid") in the status line until the next edit. It stands beside an Export-cancelled
    /// note (AF10, AF11), which stays; the encoding note went with the undo's `reset_type_follow`.
    fn undo_noted(&mut self, verb: &str, label: &str) {
        self.drop_undo_note();
        let n = format!("{verb}: {label}.");
        // In front of the normal status text, not in place of it; the plan the step started rewrites the counts.
        let counts = format!("{} textures, {} sets", self.items.len(), self.sets.len());
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
        self.apply_snapshot(&c.before);
        self.undo_noted("Undid", &c.label);
    }

    /// Redo one step. A step that created a file copies it again from its source.
    pub fn redo(&mut self) {
        let Some(c) = self.history.redo_step() else {
            return;
        };
        if let Some(cf) = &c.created_file {
            if let Err(e) = std::fs::copy(&cf.source, &cf.path) {
                self.history.refuse_redo();
                self.dialog = Some(Dialog::Error(format!(
                    "could not copy {} again: {e}",
                    cf.source.display()
                )));
                return;
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

    /// The preset a plan uses without the project's: the user config's, else the target's default.
    fn fallback_preset(&self) -> String {
        self.user_config
            .as_deref()
            .and_then(|p| ConfigFile::load(p).ok())
            .and_then(|c| c.preset)
            .unwrap_or_else(|| engine::presets::default_preset_for(self.target).to_string())
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
                "NVTT {} at {}: untested (3.2.5 is), acknowledge ENC-NVTT-VERSION to use it",
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
            || self
                .sets_doc
                .as_ref()
                .is_some_and(crate::sets_doc::SetsDoc::is_dirty)
    }

    /// Raise the dialog for a failed save of `kind`; the error text names the file.
    fn save_failed(&mut self, kind: DocKind, e: &SaveError) {
        self.dialog = Some(match e {
            SaveError::ChangedOnDisk => Dialog::ExternalChange(kind),
            e => Dialog::Error(format!("{}: {e}", kind.file())),
        });
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
        let mut lines = Vec::new();
        lines.extend(self.kept_banner.clone());
        if let Some(id) = self.missing_preset() {
            lines.push(format!(
                "preset {id} not found: using {}",
                self.fallback_preset()
            ));
        }
        if let Some(e) = self.project.as_ref().and_then(ProjectDoc::error) {
            lines.push(read_only_banner(e));
        }
        if let Some(e) = self.sets_doc.as_ref().and_then(|s| s.error()) {
            lines.push(format!(
                "{e} — sets and PBR JSON settings are read-only until it is fixed and reloaded"
            ));
        }
        self.banner = if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        };
    }

    /// Reload both documents from disk (the banner's Reload), then plan once.
    pub fn reload_documents(&mut self) {
        self.dialog = None;
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
            wrote = true;
        }
        if wrote {
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
                    if let Err(e) = p.save(true) {
                        self.save_failed(which, &e);
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
                    if let Err(e) = s.save(true) {
                        self.save_failed(which, &e);
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
        } else if self.is_dirty() {
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
        } else if self.is_dirty() {
            self.dialog = Some(Dialog::CloseUnsaved);
            false
        } else {
            true
        }
    }
}

/// Delete a file a command created, if it still has the SHA-256 it was created with.
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
