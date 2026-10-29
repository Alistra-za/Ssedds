//! The NIF edit session (spec §3.4, §7, §8.1; plan A16): the source copy an edit applies to, its edit document and
//! undo stack, the patched bytes and their model, and the live `Parsed` preview in the Meshes 3D panel. No egui types.
use super::edit_doc::{self, DocOp, EditDoc, ExportRec, NifHistory, OpStatus, SourceId};
use super::export::{placement, read_back, DocBadge, ExportOutcome, GuardChoice, Placement};
use super::pg_entries::{candidates, pg_ignore, simulate_shape, PgFile, PGP_ENV_SCALE, PG_IGNORED};
use super::worklist::{NifProblem, NifTab};
use super::{db_err, texture_sources, MeshesModel, ScanTarget};
use crate::model::viewport3d::{read_file, Clock, Content, FileSource};
use engine::dds::Kept;
use modscan::mo2::Instance;
use modscan::store::{PathCopy, ShapeView, KIND_BSA};
use nif::anim::{apply_anim, clip_tracks_in, free_tracks_in, Track};
use nif::edit::validate::{
    demote_source, new_errors, validate, validate_ops, Level, Target, Warning,
};
use nif::edit::{Anchor, Op, Outcome};
use nif::fields::Field;
use nif::keys::KeyData;
use nif::material::{MaterialState, SLSF2_TRUE_PBR};
use nif::model::{read_model, NifModel, ShapeModel};
use render3d::{ParsedNif, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// What the edit panel shows: the root's extra data or one shape (by block).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Root,
    Shape(u32),
}

/// The panel line when a controller's times are not finite (B-P33): no clock, no Time slider.
pub const ANIM_INVALID: &str = "This animation has invalid times: playback is off.";

/// What the preview animates (v2 B3): the free controller chains, or one manager clip by its sequence block (the
/// Controllers tab's Preview selector, B7). A clip pauses the free chains: they show the file value (E6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playing {
    #[default]
    Free,
    Clip(u32),
}

/// Origin-name hints of tool-generated output (OD3): editable, with a warning.
pub const GENERATED_HINTS: [&str; 7] = [
    "dyndolod",
    "texgen",
    "xlodgen",
    "bodyslide",
    "grass",
    "synthesis",
    "nemesis",
];

pub struct NifEdit {
    pub doc: EditDoc,
    pub history: NifHistory,
    /// Where the document is saved: the export folder's, else `autosave` (Note GD5). Empty with neither (a test's
    /// read-only session): nothing is saved.
    pub doc_path: PathBuf,
    /// Both empty without an export folder: export is blocked with `NO_EXPORT_FOLDER`.
    pub export_path: PathBuf,
    pub edit_mod: PathBuf,
    /// Note GD5 (c): the app-folder document of this NIF used while no export folder is chosen; removed by the first
    /// save into a folder.
    pub autosave: Option<PathBuf>,
    pub source: Arc<Vec<u8>>,
    pub source_copy: PathCopy,
    pub data_path: String,
    pub patched: Arc<Vec<u8>>,
    /// Note GD2 (CQ1-CQ4): what the export writes, `patched` without its orphan blocks; rebuilt by `reapply`.
    pub out: Arc<Vec<u8>>,
    /// How many blocks `out` dropped; None when the cleanup could not run (`cleanup_skip` says why).
    pub compaction: Option<usize>,
    pub cleanup_skip: Option<String>,
    /// The source's block count: blocks past it are ones the edits appended.
    source_blocks: u32,
    /// A process-unique id of the current edit state, new on every apply, undo, redo, revert and reload (Note CU2):
    /// the refusal lines key on it, not on the `Arc` address, which an allocation can reuse.
    pub rev: u64,
    pub model: NifModel,
    pub outcomes: Vec<Outcome>,
    pub warnings: Vec<Warning>,
    /// validate_ops over the document's ops (B4/B5 key errors), recomputed with the ops in `reapply`.
    pub op_warnings: Vec<Warning>,
    /// B-P41: `PGP_ENV_SCALE` Warnings from the cached In game simulation (never blocking, never a new simulation).
    pub pg_warnings: Vec<Warning>,
    /// Every controller's editable keys by (controller, clip), `nif::edit::keys_of`; rebuilt in `reapply`, never
    /// per frame (F23). B7 adds the clip entries.
    pub keys: BTreeMap<(u32, Option<u32>), Result<KeyData, String>>,
    pub source_warnings: Vec<Warning>,
    pub selection: Selection,
    pub solo: bool,
    pub check_exists: bool,
    /// (applied, conflicts) when the source hash differed from the document's.
    pub source_changed: Option<(usize, usize)>,
    /// OD3 warning text.
    pub generated: Option<String>,
    pub le_mesh: bool,
    /// The last failure (apply, save, undo text); `MeshesModel` moves it to its own `error`.
    pub error: Option<String>,
    /// Every enabled copy of the NIF (the store's order), for the placement check and the PGPatcher re-run note.
    pub copies: Vec<PathCopy>,
    /// The shape cards of the edited bytes (`shape_cards`), rebuilt by every `reapply` (Note CZ1; was the scan's
    /// cards of `source_copy`, final review I1).
    pub cards: Vec<ShapeView>,
    /// Meshes UI T2: each shape's controllers line ("2: Emissive multiple, Alpha (clip)"), by shape block; rebuilt with
    /// `cards`.
    pub card_ctrls: HashMap<u32, String>,
    /// The placement check (spec §8.2); None without an MO2 instance.
    pub placement: Option<Placement>,
    /// Why "Export NIF" is disabled, from the in-memory state (`export_block`), kept current by every change.
    pub blocked: Option<String>,
    /// Note GD1: this edit's worklist rows, rebuilt with `blocked` (a new `Arc` only when they change, rule 9.1).
    pub problems: Arc<Vec<NifProblem>>,
    /// Note CS: what a shader type change also did to the flags, until the next change.
    pub notice: Option<String>,
    /// The guard dialog's question, while it is open.
    pub guard: Option<GuardAsk>,
    /// The last export's result line, and whether it is an error.
    pub export_note: Option<(String, bool)>,
    /// Note CQ5: "Exported without cleanup: <reason>." after an export that wrote the patch uncompacted.
    pub cleanup_note: Option<String>,
    /// Meshes UI T3: the last export wrote exactly these ops ("Export NIF again"); kept here so the panel never hashes
    /// the ops per frame.
    pub up_to_date: bool,
    /// Meshes UI T5b: the other weight file (`x_0.nif` ↔ `x_1.nif`) when the scan has it, set when the session opens.
    /// The game uses `_1` only at weight 100, so a heel height belongs in both; the editor edits one file at a time.
    pub weight_sibling: Option<String>,
    /// A process-unique id of this session (T5b review Important 1): UI state that belongs to one session keys on it.
    pub session: u64,
    /// `nif::edit::apply` failed: the patched bytes are stale.
    apply_err: Option<String>,
    /// `render3d::parse(&patched)`, once per change.
    parsed: Arc<ParsedNif>,
    /// What the preview animates.
    pub playing: Playing,
    /// The picked clip's name (Note CU1): a revert can move another clip onto its block.
    playing_name: Option<String>,
    /// Per animated shape: its parsed material and its tracks for `playing`. Rebuilt by `reapply` and
    /// `set_playing`, never per frame (F22, F23).
    anim: Vec<(ShapeId, MaterialState, Vec<Track>)>,
    /// The clock time whose materials the viewport shows; None after anything sent un-animated materials.
    anim_shown: Option<f32>,
    /// The shapes the last `tick_anim` animated (D15: one that leaves gets its file material back).
    anim_last: Vec<ShapeId>,
    /// v2 B8 fix F1: per shape, the files that can patch it (keyed on its slots and the JSON generation) and its
    /// last simulation (keyed on its parsed material, the JSON generation and the scan). The NIF path is the
    /// session's own, so it needs no key.
    sim_cache: BTreeMap<ShapeId, SimCache>,
    /// `simulate_shape` calls so far (the cache's misses).
    pub sim_runs: usize,
    /// Entries `shape_matches` tested to find candidates (fix round 2: the index keeps this far below a full scan).
    pub match_tests: usize,
    /// v2 B8: In game (simulated) is showing: each shape's simulated material and its lines. None on Edited source.
    /// `preview_nif` draws these materials, so `materials()` and every `before` follow the shown side.
    pub in_game: Option<Vec<(ShapeId, MaterialState, Vec<String>)>>,
    /// A merged (drag) push not saved yet.
    unsaved: bool,
    /// The merge key of the drag whose ops are the last ones, and how many; a push with the same key and count
    /// replaces those ops.
    drag: Option<(String, usize)>,
}

/// One shape's cached In game work (fix F1): the candidate files for (slots, JSON generation) and the simulation
/// for (parsed material, JSON generation, scan revision).
#[derive(Default)]
struct SimCache {
    #[allow(clippy::type_complexity)]
    cands: Option<(Vec<nif::path::TexPath>, u64, Vec<(usize, Vec<usize>)>)>,
    #[allow(clippy::type_complexity)]
    out: Option<(MaterialState, u64, u64, (MaterialState, Vec<String>))>,
}

/// Under the toggle when `show_edit_preview` failed and a plain copy shows (fix F8).
pub const EDIT_HIDDEN: &str =
    "Your edit could not be shown. Switch to In game (simulated) or Edited source to retry.";

/// The open guard dialog: the block diff, whether Diff is showing it, and the hash of the file it was asked about
/// (Overwrite applies only while the file still has it).
#[derive(Debug, Clone, PartialEq)]
pub struct GuardAsk {
    pub diff: Vec<String>,
    pub show_diff: bool,
    pub sha256: String,
    /// sdc never exported to this path: the file there is not one it wrote and then saw changed.
    pub never_exported: bool,
}

/// The idle reason Export NIF is off: drawn neutral, not as a block (review Minor 5).
pub const NO_EDITS: &str = "No edits to export.";
pub const RERUN_PGPATCHER: &str = "Re-run PGPatcher so its output includes this edit.";
/// No export folder chosen: the edit works and autosaves, its export waits (Note GD5); FB1's reason, word for word.
pub const NO_EXPORT_FOLDER: &str = crate::model::review::NO_EXPORT_FOLDER;
/// Note GD2: why `export_nif_edits` skips a row with a file Tessalith did not write.
pub const NIF_UNANSWERED: &str =
    "the file on disk was not written by Tessalith: answer Keep or Overwrite on its row";
pub const NIF_KEPT: &str = "you kept the file on disk";
pub const NIF_CHANGED_SINCE: &str = "changed on disk since you answered";
/// Note GD2 (was CQ5's under-button line): the export will write the patch without dropping orphan blocks.
pub const NO_CLEANUP: &str =
    "Exports without cleanup: the file has a block Tessalith does not understand";

/// Note GD2: one NIF with pending edits, as the Review lists it. `foreign`: SHA-256 of the file at the export path
/// when Tessalith did not write it (the row waits for an answer); `answer`: the user's, while that file still has
/// the hash it was given for; `compaction`: the orphan blocks the export drops (None when it cannot clean up).
#[derive(Debug, Clone, PartialEq)]
pub struct NifReviewRow {
    pub path: String,
    pub mod_name: String,
    pub blocked: Option<String>,
    pub foreign: Option<[u8; 32]>,
    pub answer: Option<GuardChoice>,
    pub compaction: Option<usize>,
}

/// `foreign_cache`'s key: the export path, the last export's hash and the file's (length, modified time).
pub(super) type ForeignKey = (
    PathBuf,
    Option<String>,
    Option<(u64, Option<std::time::SystemTime>)>,
);
pub(super) type ForeignCache = Option<(ForeignKey, Result<Option<[u8; 32]>, String>)>;

/// The copy an edit applies to: the first that is neither PGPatcher output nor our own edit mod (spec §8.1).
pub fn pick_source<'a>(copies: &'a [PathCopy], edit_mod: &Path) -> Option<&'a PathCopy> {
    copies
        .iter()
        .find(|c| !c.derived && !same_dir(Path::new(&c.root), edit_mod))
}

pub fn generated_warning(origin: &str) -> Option<String> {
    let o = origin.to_ascii_lowercase();
    GENERATED_HINTS
        .iter()
        .any(|h| o.contains(h))
        .then(|| format!("{origin} is generated by a tool: its next run will overwrite this edit."))
}

const SLSF1_ENVIRONMENT_MAPPING: u32 = 1 << 7;
const SLSF1_MODEL_SPACE_NORMALS: u32 = 1 << 12;
const SLSF2_GLOW_MAP: u32 = 1 << 6;
const SLSF2_BACK_LIGHTING: u32 = 1 << 27;
/// The CS/PGPatcher TruePBR slot meanings, as the set editor's slot kinds name them.
const PBR_SLOTS: [&str; 9] = [
    "Diffuse",
    "Normal",
    "Emissive",
    "Height",
    "Unused",
    "RMAOS",
    "Coat / Fuzz",
    "Subsurface",
    "Unused",
];

/// The Shader tab's name of texture-set slot `slot` (spec §7, F§1.3).
pub fn edit_slot_label(shader_type: u32, slsf1: u32, slsf2: u32, slot: usize) -> &'static str {
    if slsf2 & SLSF2_TRUE_PBR != 0 {
        return PBR_SLOTS.get(slot).copied().unwrap_or("Unused");
    }
    match (slot, shader_type) {
        (0, _) => "Diffuse",
        (1, _) => "Normal",
        (2, 5) => "Skin",
        (2, 6) => "Hair",
        (2, 2) => "Glow",
        (2, _) if slsf2 & SLSF2_GLOW_MAP != 0 => "Glow",
        (2, _) => "Rim / back-light",
        (3, 4) => "Detail",
        (3, _) => "Height",
        (4, 16) => "Eye env cube",
        (5, 16) => "Eye env mask",
        (4, 1) => "Env cube",
        (5, 1) => "Env mask",
        (4, _) if slsf1 & SLSF1_ENVIRONMENT_MAPPING != 0 => "Env cube",
        (5, _) if slsf1 & SLSF1_ENVIRONMENT_MAPPING != 0 => "Env mask",
        (4, _) => "Env cube",
        (5, _) => "Env mask",
        (6, 11) => "Inner layer",
        (6, 4) => "Face tint",
        (6, _) => "Inner layer / tint",
        (7, _) if slsf2 & SLSF2_BACK_LIGHTING != 0 => "Back-light",
        (7, _) if slsf1 & SLSF1_MODEL_SPACE_NORMALS != 0 => "Model-space specular",
        (7, _) => "Back-light / specular",
        _ => "Unused",
    }
}

/// A field in words, as the tabs name it (Meshes UI T3: no `Slsf1` or `UvOffset(0)` in the pending list).
pub fn field_label(f: Field) -> String {
    let axis = |n: u8| ["U", "V"].get(n as usize).copied().unwrap_or("?");
    let rgb = |n: u8| ["R", "G", "B"].get(n as usize).copied().unwrap_or("?");
    match f {
        Field::ShaderType => "Shader type".into(),
        Field::Slsf1 => "SLSF1 flags".into(),
        Field::Slsf2 => "SLSF2 flags".into(),
        Field::UvOffset(n) => format!("UV offset {}", axis(n)),
        Field::UvScale(n) => format!("UV scale {}", axis(n)),
        Field::EmissiveColor(n) => format!("Emissive colour {}", rgb(n)),
        Field::EmissiveMultiple => "Emissive multiple".into(),
        Field::ClampMode => "Clamp mode".into(),
        Field::Alpha => "Shader alpha".into(),
        Field::Refraction => "Refraction strength".into(),
        Field::Glossiness => "Glossiness".into(),
        Field::SpecularColor(n) => format!("Specular colour {}", rgb(n)),
        Field::SpecularStrength => "Specular strength".into(),
        Field::LightingEffect1 => "Lighting effect 1".into(),
        Field::LightingEffect2 => "Lighting effect 2".into(),
        // `field_name` names it from the shader type (Note GD1); this is the fallback for a type without names.
        Field::Trailer(n) => format!("Shader type value {}", n + 1),
        Field::AlphaFlags => "Alpha flags".into(),
        Field::AlphaThreshold => "Alpha threshold".into(),
        Field::CtrlFlags => "Controller flags".into(),
        Field::Frequency => "Frequency".into(),
        Field::Phase => "Phase".into(),
        Field::StartTime => "Start".into(),
        Field::StopTime => "Stop".into(),
        Field::Variable => "Controlled variable".into(),
        Field::InterpValue(n) => format!("Interpolator value {}", n + 1),
    }
}

/// Field `f` of block `block` in words: a trailer value by its name for the block's shader type (Note GD1).
pub fn field_name(m: &NifModel, block: u32, f: Field) -> String {
    let trailer = |n: u8| {
        let l = m.lighting.get(&block)?;
        l.lighting.params.trailer.names().get(n as usize).copied()
    };
    match f {
        Field::Trailer(n) => trailer(n).map_or_else(|| field_label(f), str::to_string),
        _ => field_label(f),
    }
}

/// A short name of what an op changes, for the pending list's and the worklist's rows. `m` is the model the op
/// applied to (its anchor's block is current after `reapply`).
pub fn op_label(op: &Op, m: &NifModel) -> String {
    let (what, at) = match op {
        Op::SetField { at, field, .. } | Op::SetBits { at, field, .. } => {
            (field_name(m, at.block, *field), at)
        }
        Op::SetTexturePath { at, slot, .. } => (format!("Texture slot {slot}"), at),
        Op::SplitAlpha { at } => ("Split alpha".into(), at),
        Op::AddAlpha { at, .. } => ("Add alpha".into(), at),
        Op::RemoveAlpha { at } => ("Remove alpha".into(), at),
        Op::AddExtra { at, name, .. } => (format!("Add extra data {name}"), at),
        Op::EditExtra { at, .. } => ("Edit extra data".into(), at),
        Op::RemoveExtra { at } => ("Remove extra data".into(), at),
        Op::SetKeys { at, .. } => ("Edit keys".into(), at),
        Op::AddController { at, .. } => ("Add controller".into(), at),
        Op::RemoveController { at } => ("Remove controller".into(), at),
        Op::AddSequence { at, name, .. } => (format!("Add clip {name}"), at),
        Op::RemoveSequence { at, name, .. } => (format!("Remove clip {name}"), at),
        Op::RenameSequence {
            at, name, new_name, ..
        } => (format!("Rename clip {name} to {new_name}"), at),
        Op::SetSequenceTiming { at, name, .. } => (format!("Set clip timing {name}"), at),
        Op::ConvertChainToClips { at, name } => (format!("Convert to clip {name}"), at),
    };
    match &at.shape {
        Some(s) => format!("{s}: {what}"),
        None => what,
    }
}

/// The undo step's name for an op (Note GD4): "Set shader type", "Remove controller".
pub fn op_name(op: &Op) -> String {
    match op {
        Op::SetField { field, .. } | Op::SetBits { field, .. } => {
            let f = field_label(*field);
            // "Shader type" reads "Set shader type"; an initialism ("SLSF1 flags", "UV offset U") keeps its case.
            let mut c = f.chars();
            match (c.next(), c.next()) {
                (Some(a), Some(b)) if b.is_lowercase() => {
                    format!("Set {}{}", a.to_lowercase(), &f[a.len_utf8()..])
                }
                _ => format!("Set {f}"),
            }
        }
        Op::SetTexturePath { .. } => "Set texture path".into(),
        Op::SplitAlpha { .. } => "Split alpha".into(),
        Op::AddAlpha { .. } => "Add alpha".into(),
        Op::RemoveAlpha { .. } => "Remove alpha".into(),
        Op::AddExtra { .. } => "Add extra data".into(),
        Op::EditExtra { .. } => "Edit extra data".into(),
        Op::RemoveExtra { .. } => "Remove extra data".into(),
        Op::SetKeys { .. } => "Edit keys".into(),
        Op::AddController { .. } => "Add controller".into(),
        Op::RemoveController { .. } => "Remove controller".into(),
        Op::AddSequence { .. } => "Add clip".into(),
        Op::RemoveSequence { .. } => "Remove clip".into(),
        Op::RenameSequence { .. } => "Rename clip".into(),
        Op::SetSequenceTiming { .. } => "Set clip timing".into(),
        Op::ConvertChainToClips { .. } => "Convert to clip".into(),
    }
}

/// The shape's lighting shader's LSCF var-8 (Env map scale) controller, if its chain has one.
fn env_scale_ctrl(m: &NifModel, sm: &ShapeModel) -> Option<u32> {
    let l = m.lighting.get(&sm.shader?)?;
    l.prop.ctrls.iter().copied().find(|c| {
        m.ctrls.get(c).is_some_and(|c| {
            &*c.class == "BSLightingShaderPropertyFloatController" && c.var == Some(8)
        })
    })
}

fn anchor_mut(op: &mut Op) -> &mut Anchor {
    match op {
        Op::SetField { at, .. }
        | Op::SetBits { at, .. }
        | Op::SetTexturePath { at, .. }
        | Op::SplitAlpha { at }
        | Op::AddAlpha { at, .. }
        | Op::RemoveAlpha { at }
        | Op::AddExtra { at, .. }
        | Op::EditExtra { at, .. }
        | Op::RemoveExtra { at }
        | Op::AddController { at, .. }
        | Op::RemoveController { at }
        | Op::AddSequence { at, .. }
        | Op::RemoveSequence { at, .. }
        | Op::RenameSequence { at, .. }
        | Op::SetSequenceTiming { at, .. }
        | Op::ConvertChainToClips { at, .. }
        | Op::SetKeys { at, .. } => at,
    }
}

/// `op` (a later tick of the same drag as `old`) takes `old`'s base: the value the drag started from.
fn keep_base(op: &mut Op, old: &Op) {
    match (op, old) {
        (Op::SetField { base, .. }, Op::SetField { base: b, .. }) => *base = *b,
        (Op::SetBits { base, .. }, Op::SetBits { base: b, .. }) => *base = *b,
        (Op::SetTexturePath { base, .. }, Op::SetTexturePath { base: b, .. }) => base.clone_from(b),
        (Op::EditExtra { base, .. }, Op::EditExtra { base: b, .. }) => base.clone_from(b),
        (Op::SetKeys { base, .. }, Op::SetKeys { base: b, .. }) => base.clone_from(b),
        _ => {}
    }
}

impl NifEdit {
    /// Opens `source`, the bytes of `source_copy`, at `data_path`. `edit_mod` None: the document lives at `autosave`
    /// (Note GD5) and export is blocked; neither: read-only with an in-memory document. A saved document is loaded
    /// (Err when it is malformed or newer: it is never overwritten); when its source hash differs, its ops are
    /// re-anchored on the new source and `source_changed` counts the outcomes.
    pub fn open(
        source: Vec<u8>,
        source_copy: PathCopy,
        data_path: &str,
        edit_mod: Option<&Path>,
        autosave: Option<&Path>,
    ) -> Result<NifEdit, String> {
        edit_doc::win(data_path)?; // read-only too: the session never opens on such a path
        let (scan, model) = read_source(&source)?;
        let id = source_id(&source_copy, data_path, &source);
        let (edit_mod, doc_path, export_path) = match edit_mod {
            Some(m) => (
                m.to_path_buf(),
                edit_doc::doc_path(m, data_path)?,
                edit_doc::export_path(m, data_path)?,
            ),
            None => (
                PathBuf::new(),
                autosave.map(Path::to_path_buf).unwrap_or_default(),
                PathBuf::new(),
            ),
        };
        let mut doc = match doc_path.as_os_str().is_empty() {
            true => None,
            false => edit_doc::load(&doc_path)?,
        }
        .unwrap_or_else(|| EditDoc::new(id.clone()));
        let changed = doc.source.sha256 != id.sha256;
        if changed {
            doc.source = id; // saved with the next change; the statuses carry the conflicts
        }
        let mut e = NifEdit {
            doc,
            history: NifHistory::default(),
            doc_path,
            export_path,
            edit_mod,
            autosave: autosave.map(Path::to_path_buf),
            source_blocks: block_count(&source),
            source: Arc::new(source),
            data_path: data_path.into(),
            patched: Arc::new(vec![]),
            out: Arc::new(vec![]),
            compaction: None,
            cleanup_skip: None,
            rev: 0,
            source_warnings: validate(&model, None),
            model,
            outcomes: vec![],
            warnings: vec![],
            op_warnings: vec![],
            pg_warnings: vec![],
            keys: BTreeMap::new(),
            selection: Selection::Root,
            solo: false,
            check_exists: false,
            source_changed: None,
            generated: generated_warning(&source_copy.origin),
            source_copy,
            le_mesh: scan.le_mesh,
            error: None,
            copies: vec![],
            cards: vec![],
            card_ctrls: HashMap::new(),
            placement: None,
            blocked: None,
            problems: Arc::default(),
            notice: None,
            guard: None,
            export_note: None,
            cleanup_note: None,
            up_to_date: false,
            weight_sibling: None,
            session: next_key(),
            apply_err: None,
            parsed: Arc::default(),
            playing: Playing::Free,
            playing_name: None,
            anim: vec![],
            anim_shown: None,
            anim_last: vec![],
            sim_cache: BTreeMap::new(),
            sim_runs: 0,
            match_tests: 0,
            in_game: None,
            unsaved: false,
            drag: None,
        };
        // Applying re-anchors each op on the current source: `nif::edit::rebase` and the patched bytes in one pass.
        e.reapply();
        if changed {
            e.count_changed();
        }
        Ok(e)
    }

    fn count_changed(&mut self) {
        let n = self
            .doc
            .ops
            .iter()
            .filter(|d| d.status == OpStatus::Conflict)
            .count();
        self.source_changed = Some((self.doc.ops.len() - n, n));
    }

    /// The source copy's bytes changed on disk (export step 1): the ops are re-anchored on them and the statuses
    /// updated and saved.
    pub fn reload_source(&mut self, source: Vec<u8>) -> Result<(), String> {
        let (scan, model) = read_source(&source)?;
        self.doc.source = source_id(&self.source_copy, &self.data_path, &source);
        self.source_warnings = validate(&model, None);
        self.le_mesh = scan.le_mesh;
        self.source_blocks = block_count(&source);
        self.source = Arc::new(source);
        self.reapply();
        self.count_changed();
        self.save();
        Ok(())
    }

    /// Why the current state cannot be exported, from memory alone (export steps 2-4): refused ops first (A16 keeps
    /// them as conflicts), then conflicts, new validation errors and bytes that do not read back.
    pub fn export_block(&self) -> Option<String> {
        if self.edit_mod.as_os_str().is_empty() {
            return Some(NO_EXPORT_FOLDER.into());
        }
        if let Some(e) = &self.apply_err {
            return Some(e.clone());
        }
        if self.doc.ops.is_empty() && self.doc.export.sha256.is_none() {
            return Some(NO_EDITS.into());
        }
        let refused = self
            .outcomes
            .iter()
            .filter(|o| matches!(o, Outcome::Refused(_)))
            .count();
        if refused > 0 {
            let s = if refused == 1 { "edit" } else { "edits" };
            return Some(format!("{refused} {s} cannot be applied."));
        }
        if self.doc.has_conflicts()
            || self
                .outcomes
                .iter()
                .any(|o| matches!(o, Outcome::Conflict(_)))
        {
            return Some("Resolve the conflicts first.".into());
        }
        if let Some(w) = new_errors(&self.source_warnings, &self.warnings).first() {
            return Some(w.text.clone());
        }
        // Note GD8: a Warn (bad clip timing) holds back only what it names
        if let Some(w) = self.op_warnings.iter().find(|w| w.level == Level::Error) {
            return Some(w.text.clone());
        }
        read_back(&self.patched).err()
    }

    /// Recomputes `blocked` after the ops or the warnings changed.
    pub fn refresh_blocked(&mut self) {
        self.blocked = self.export_block();
        self.refresh_problems();
    }

    /// The worklist rows (Note GD1), replaced only when they differ, so a cache keyed on the `Arc` holds.
    fn refresh_problems(&mut self) {
        let p = super::worklist::problems(self);
        if *self.problems != p {
            self.problems = Arc::new(p);
        }
    }

    /// Export steps 1-6 (spec §8.3). Nothing is written when a check fails; a file sdc did not write is replaced
    /// only by an Overwrite answered for exactly the bytes on disk now.
    pub fn export(&mut self, choice: Option<GuardChoice>) -> ExportOutcome {
        let asked = self.guard.take();
        if choice == Some(GuardChoice::KeepTheirs) {
            return ExportOutcome::KeptTheirs; // whatever is on disk now stays
        }
        let overwrite = match choice {
            Some(GuardChoice::Overwrite) => asked.map(|a| a.sha256),
            _ => None,
        };
        self.write_out(overwrite)
    }

    /// [`Self::export`]'s steps with the answer already given (Note GD2's Review path too): `overwrite` is the hash
    /// (hex) of the file on disk the user answered Overwrite for; a foreign file with another hash is not written.
    pub(super) fn write_out(&mut self, overwrite: Option<String>) -> ExportOutcome {
        if self.edit_mod.as_os_str().is_empty() {
            return ExportOutcome::Blocked(NO_EXPORT_FOLDER.into());
        }
        let c = &self.source_copy;
        let now = match read_file(&FileSource::from_store(c.kind, &c.root, &self.data_path)) {
            Ok(b) => b,
            Err(e) => return ExportOutcome::Blocked(e),
        };
        if edit_doc::sha256_hex(&now) != self.doc.source.sha256 {
            if let Err(e) = self.reload_source(now) {
                return ExportOutcome::Blocked(e);
            }
        }
        self.refresh_blocked();
        if let Some(why) = self.blocked.clone() {
            return ExportOutcome::Blocked(why);
        }
        // Note CQ: the file drops the blocks our edits appended that nothing reaches; on any doubt it is the patch
        // (`reapply` made both, Note GD2)
        let (out, skipped) = (self.out.to_vec(), self.cleanup_skip.clone());
        let path = self.export_path.clone();
        let cannot_read = |e: std::io::Error| {
            ExportOutcome::Blocked(format!("Cannot read {}: {e}", path.display()))
        };
        let last = self.doc.export.sha256.clone();
        let ours = |b: &[u8]| last.as_deref() == Some(edit_doc::sha256_hex(b).as_str());
        match engine::dds::may_replace(&path, &out, ours) {
            Ok(()) => {}
            Err(Kept::Unreadable(e)) => return cannot_read(e),
            Err(Kept::Foreign) => {
                let on_disk = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => return cannot_read(e),
                };
                let sha256 = edit_doc::sha256_hex(&on_disk);
                match overwrite {
                    Some(h) if h == sha256 => {}
                    _ => {
                        let diff = nif::write::block_diff(&on_disk, &out).unwrap_or_else(|e| {
                            vec![format!("The file on disk does not read as a NIF: {e}")]
                        });
                        self.guard = Some(GuardAsk {
                            diff: diff.clone(),
                            show_diff: false,
                            sha256,
                            never_exported: last.is_none(),
                        });
                        return ExportOutcome::Guard { diff };
                    }
                }
            }
        }
        if let Err(e) = engine::dds::write_atomic(&path, &out) {
            return ExportOutcome::Blocked(format!("Cannot write {}: {e}", path.display()));
        }
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.doc.export = ExportRec {
            sha256: Some(edit_doc::sha256_hex(&out)),
            at: Some(at),
            ops_hash: Some(self.doc.ops_hash()),
        };
        self.up_to_date = true;
        self.save();
        self.cleanup_note = skipped.map(|why| format!("Exported without cleanup: {why}."));
        ExportOutcome::Written {
            rerun_pgpatcher: self.copies.iter().any(|c| c.derived),
        }
    }

    /// Applies the ops to the source: patched bytes, outcomes (each `ReAnchored` block written back into its op),
    /// statuses (a conflict or a refusal blocks export), the model, the warnings and the parsed preview.
    fn reapply(&mut self) {
        self.rev = next_key();
        self.notice = None;
        let a = match nif::edit::apply(&self.source, &self.doc.ops()) {
            Ok(a) => a,
            Err(e) => {
                self.error = Some(e.to_string());
                self.apply_err = Some(e.to_string());
                self.up_to_date = false; // review Minor 3: the stale bytes are not what the last export wrote
                self.refresh_blocked();
                return;
            }
        };
        self.apply_err = None;
        self.export_note = None; // the last export's line no longer describes these edits
        self.cleanup_note = None;
        for (d, o) in self.doc.ops.iter_mut().zip(&a.outcomes) {
            if let Outcome::ReAnchored { block } = o {
                anchor_mut(&mut d.op).block = *block;
            }
            d.status = match o {
                Outcome::Conflict(_) | Outcome::Refused(_) => OpStatus::Conflict,
                Outcome::Applied | Outcome::ReAnchored { .. } => OpStatus::Applied,
            };
        }
        self.patched = Arc::new(a.bytes);
        // Note GD2: the export's bytes once per change, so the Review row can count what the cleanup drops
        match nif::edit::compact(&self.patched, self.source_blocks) {
            Ok(out) => {
                let n = block_count(&self.patched).saturating_sub(block_count(&out));
                self.compaction = Some(n as usize);
                self.out = Arc::new(out);
                self.cleanup_skip = None;
            }
            Err(why) => {
                self.out = self.patched.clone();
                self.compaction = None;
                self.cleanup_skip = Some(why);
            }
        }
        self.outcomes = a.outcomes;
        self.up_to_date = !self.doc.pending() && self.doc.export.sha256.is_some();
        match read_model(&self.patched) {
            Ok(m) => self.model = m,
            Err(e) => self.error = Some(e.to_string()),
        }
        self.cards = shape_cards(&self.patched); // once per change, never per frame (rule 9.1)
        self.card_ctrls = card_ctrls(&self.model);
        self.warnings = validate(&self.model, None);
        demote_source(&self.source_warnings, &mut self.warnings);
        // After the ReAnchored write-back: each op's `at.block` is the block its warning targets.
        self.op_warnings = validate_ops(&self.doc.ops());
        // B-P41: one header parse per change; one per controller cost 266 ms a drag tick on a 608-controller NIF.
        let header = nif::header::Header::parse(&self.patched).map_err(|e| e.to_string());
        self.keys =
            self.model
                .ctrls
                .keys()
                .map(|&c| {
                    let k = header.as_ref().map_err(Clone::clone).and_then(|h| {
                        nif::edit::keys_of_in(h, &self.patched, &self.model, c, None)
                    });
                    ((c, None), k)
                })
                .collect();
        // v2 B7: the keys of each clip a managed controller is driven by (once per change).
        // shortcut: clips_of_in walks every property per (sequence, controller): ~18 ms a drag tick on a 608-controller,
        // 818-shape NIF (CM32); invert it per sequence if a file the owner edits passes 16 ms.
        if let Ok(h) = &header {
            for &c in self.model.ctrls.keys().filter(|&&c| self.model.managed(c)) {
                for seq in nif::anim::clips_of_in(&self.model, h, c) {
                    let k = nif::edit::keys_of_in(h, &self.patched, &self.model, c, Some(seq));
                    self.keys.insert((c, Some(seq)), k);
                }
            }
        }
        self.parsed = Arc::new(render3d::parse(&self.patched));
        // A clip the edits or a source reload removed: back to the free chains (with the last sequence gone, the
        // Preview selector is gone too). Another name at its block is another clip a revert moved there (Note CU1).
        if let Playing::Clip(seq) = self.playing {
            let name = &self.playing_name;
            if !self
                .model
                .sequences
                .iter()
                .any(|s| s.block == seq && s.name.as_deref() == name.as_deref())
            {
                self.playing = Playing::Free;
            }
        }
        self.anim = anim_tracks(&self.model, &self.patched, &self.parsed, self.playing);
        self.anim_shown = None;
        self.refresh_pg_warnings();
        self.refresh_blocked();
    }

    /// B-P41: a Warning under each LSCF var-8 controller of a non-PBR shape that the last cached In game simulation
    /// (for the shape's current slots) made PBR. PG_IGNORE shapes are skipped, as the simulation skips them.
    fn refresh_pg_warnings(&mut self) {
        let m = &self.model;
        self.pg_warnings = self
            .parsed
            .shapes
            .iter()
            .filter(|s| s.material.slsf2 & SLSF2_TRUE_PBR == 0)
            .filter_map(|s| {
                let sm = m.shape(s.id.0).filter(|sm| !pg_ignore(m, sm))?;
                let block = env_scale_ctrl(m, sm)?;
                let (input, _, _, (out, _)) = self.sim_cache.get(&s.id)?.out.as_ref()?;
                (input.slots == s.material.slots && out.slsf2 & SLSF2_TRUE_PBR != 0).then(|| {
                    Warning {
                        level: Level::Warn,
                        target: Target::Block { block },
                        text: PGP_ENV_SCALE.into(),
                        fix: None,
                    }
                })
            })
            .collect();
    }

    fn save(&mut self) {
        self.unsaved = false;
        if self.doc_path.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = edit_doc::save(&self.doc_path, &self.doc) {
            self.error = Some(e);
            return;
        }
        // Note GD5 (c): saved in a folder, the app-folder autosave has done its job
        if let Some(a) = self
            .autosave
            .as_ref()
            .filter(|_| !self.edit_mod.as_os_str().is_empty())
        {
            if let Err(e) = std::fs::remove_file(a) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.error = Some(format!("{}: {e}", a.display()));
                }
            }
        }
    }

    /// Note GD5 (a), (b), (d): the export folder changed while this edit has ops. The document stays in memory and
    /// moves to `root` (or to `autosave` with none) unsaved: the switch writes nothing, the next save writes there.
    /// It takes the export record of `root`'s saved document (that record describes `root`'s file). Returns the
    /// lines for Messages; Err (nothing moved) when `root`'s saved document does not load, so it is never overwritten.
    pub fn retarget(&mut self, root: Option<&Path>) -> Result<Vec<String>, String> {
        let (edit_mod, doc_path, export_path) = match root {
            Some(r) => (
                r.to_path_buf(),
                edit_doc::doc_path(r, &self.data_path)?,
                edit_doc::export_path(r, &self.data_path)?,
            ),
            None => (
                PathBuf::new(),
                self.autosave.clone().unwrap_or_default(),
                PathBuf::new(),
            ),
        };
        let theirs = match root {
            Some(_) => edit_doc::load(&doc_path)?,
            None => None,
        };
        let mut lines = vec![];
        if theirs.is_some() {
            lines.push(format!(
                "Export folder changed: the open edit of {} replaced {}'s saved edits.",
                self.data_path,
                doc_path.display()
            ));
        }
        // (d): the old folder's document stays (FC11 lists it when its export hash is recorded); the autosave is ours
        if !self.edit_mod.as_os_str().is_empty() && self.doc_path.is_file() {
            lines.push(format!(
                "Export folder changed: the edits saved before stay in {}.",
                self.doc_path.display()
            ));
        }
        self.doc.export = theirs.map(|t| t.export).unwrap_or_default();
        (self.edit_mod, self.doc_path, self.export_path) = (edit_mod, doc_path, export_path);
        self.drag = None;
        self.history.break_merge();
        self.unsaved = true;
        self.up_to_date = !self.doc.pending() && self.doc.export.sha256.is_some();
        self.export_note = None;
        self.refresh_blocked();
        Ok(lines)
    }

    /// Appends `ops` as one undo step. A push with the same merge key as the previous one (the next tick of a
    /// drag) replaces the drag's ops instead, each keeping the `base` of the drag's op with its merge key: a drag
    /// is one op (or one op per colour channel), plus the controller's start / stop while they follow the keys
    /// (B-P44), which come and go during the drag, and the SplitAlpha that started it (Note FA10).
    pub fn push(&mut self, ops: Vec<Op>, merge: Option<String>) {
        let merging = merge.is_some();
        let n = ops.len();
        let tail = self.drag_ops(merge.as_deref().unwrap_or_default()).len();
        let same_drag = merging && tail > 0;
        // Note FA10: a drag that started with a SplitAlpha (a shared alpha) keeps it; later ticks no longer see the
        // alpha as shared and push only the field op.
        let lead = same_drag
            && matches!(
                self.doc.ops[self.doc.ops.len() - tail].op,
                Op::SplitAlpha { .. }
            )
            && !matches!(ops.first(), Some(Op::SplitAlpha { .. }));
        self.drag = merge.clone().map(|k| (k, n + usize::from(lead)));
        let label = ops.first().map(op_name).unwrap_or_default();
        self.history.record(self.doc.to_text(), &label, merge);
        // Note CU1: a rename of the picked clip keeps it picked (the block stays; its name follows)
        if let Playing::Clip(seq) = self.playing {
            for op in &ops {
                if let Op::RenameSequence {
                    name,
                    nth,
                    new_name,
                    ..
                } = op
                {
                    if nif::edit::clip_key(&self.patched, &self.model, seq)
                        == Some((name.clone(), *nth))
                    {
                        self.playing_name = Some(new_name.clone());
                    }
                }
            }
        }
        let mut old: Vec<DocOp> = match same_drag {
            true => self.doc.ops.split_off(self.doc.ops.len() - tail),
            false => vec![],
        };
        if lead {
            self.doc.ops.push(old.remove(0));
        }
        let key = |op: &Op| nif::edit::merge_key(op);
        for (i, mut op) in ops.into_iter().enumerate() {
            let same = old.get(i).filter(|d| key(&d.op) == key(&op));
            if let Some(d) = same.or_else(|| old.iter().find(|d| key(&d.op) == key(&op))) {
                keep_base(&mut op, &d.op);
            }
            self.doc.ops.push(DocOp {
                op,
                status: OpStatus::Applied,
            });
        }
        self.reapply();
        if merging {
            self.unsaved = true;
        } else {
            self.save();
        }
    }

    /// The ops of the drag with merge key `merge` while it runs (empty otherwise): their bases are the values from
    /// before the drag.
    pub fn drag_ops(&self, merge: &str) -> &[DocOp] {
        match &self.drag {
            Some((k, n)) if k == merge && self.doc.ops.len() >= *n => {
                &self.doc.ops[self.doc.ops.len() - n..]
            }
            _ => &[],
        }
    }

    /// Ends a drag: the next merged push is a new undo step, and an unsaved drag is saved.
    pub fn flush(&mut self) {
        self.drag = None;
        self.history.break_merge();
        if self.unsaved {
            self.save();
        }
    }

    fn restore(&mut self, text: Option<String>) -> bool {
        self.drag = None;
        let Some(t) = text else { return false };
        match EditDoc::from_text(&t) {
            // Undo changes the ops, not what is on disk: the export record and the source identity stay current.
            Ok(mut d) => {
                d.export = std::mem::take(&mut self.doc.export);
                d.source = self.doc.source.clone();
                self.doc = d;
            }
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        }
        self.reapply();
        self.save();
        true
    }

    pub fn undo(&mut self) -> bool {
        let t = self.history.undo(self.doc.to_text());
        self.restore(t)
    }

    pub fn redo(&mut self) -> bool {
        let t = self.history.redo(self.doc.to_text());
        self.restore(t)
    }

    /// The later SetSequenceTiming ops on the clip op `i` adds (AddSequence, Convert): reverting `i` drops them too
    /// (Note GD8, CR6). Clip names are unique on a manager ignoring ASCII case, so the name finds the clip.
    // shortcut: a Rename between the two leaves the timing op to conflict after the revert; follow renames if users
    // rename and retime a new clip and then revert it.
    pub fn timing_dependents(&self, i: usize) -> Vec<usize> {
        let added = match self.doc.ops.get(i).map(|d| &d.op) {
            Some(Op::AddSequence { name, .. } | Op::ConvertChainToClips { name, .. }) => name,
            _ => return vec![],
        };
        (i + 1..self.doc.ops.len())
            .filter(|&k| {
                matches!(&self.doc.ops[k].op, Op::SetSequenceTiming { name, .. }
                    if name.eq_ignore_ascii_case(added))
            })
            .collect()
    }

    /// Drops op `i`, and the clip timing that goes with it (one undo step).
    pub fn revert(&mut self, i: usize) {
        if i >= self.doc.ops.len() {
            return;
        }
        self.drag = None;
        let label = format!("Revert: {}", op_name(&self.doc.ops[i].op));
        self.history.record(self.doc.to_text(), &label, None);
        for k in self.timing_dependents(i).into_iter().rev() {
            self.doc.ops.remove(k);
        }
        self.doc.ops.remove(i);
        self.reapply();
        self.save();
    }

    /// A conflict resolved for my value: its base becomes the value the source has now.
    pub fn keep_mine(&mut self, i: usize) {
        let ops = match nif::edit::keep_mine(&self.source, &self.doc.ops(), i) {
            Ok(o) => o,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        self.drag = None;
        self.history.record(self.doc.to_text(), "Keep mine", None);
        for (d, op) in self.doc.ops.iter_mut().zip(ops) {
            d.op = op;
        }
        self.reapply();
        self.save();
    }

    /// A conflict resolved for the source: the op is dropped (`nif::edit::take_source`).
    pub fn take_source(&mut self, i: usize) {
        self.revert(i);
    }

    /// The parsed patched NIF as the preview draws it: In game, each shape's simulated material (v2 B8); with Solo,
    /// every shape but the selected one is hidden.
    fn preview_nif(&self) -> ParsedNif {
        let mut p = (*self.parsed).clone();
        if let Some(rows) = &self.in_game {
            for s in &mut p.shapes {
                if let Some((_, m, _)) = rows.iter().find(|(id, _, _)| *id == s.id) {
                    s.material = m.clone();
                }
            }
        }
        if self.solo {
            for s in &mut p.shapes {
                if self.selection != Selection::Shape(s.id.0) {
                    s.material.hidden = true;
                }
            }
        }
        p
    }

    /// v2 B8 (closes R11): each shape's material as PGPatcher would leave the edited NIF (`pg_entries::simulate_shape`
    /// over `files`, the JSON of read `gen`), with its lines. Shapes without a texture set keep their material and get no
    /// line; PG_IGNORE ones keep it with `PG_IGNORED`. Those two checks run every time; only the simulation is cached
    /// (fix F1, keyed on the parsed material, `gen` and the scan `rev`). From the parsed edit (not `preview_nif`).
    pub fn in_game_materials(
        &mut self,
        files: &[PgFile],
        gen: u64,
        rev: u64,
        exists: &dyn Fn(&str) -> bool,
    ) -> Vec<(ShapeId, MaterialState, Vec<String>)> {
        let nif_path = self.data_path.replace('/', "\\");
        let parsed = self.parsed.clone();
        let mut rows = Vec::with_capacity(parsed.shapes.len());
        for s in &parsed.shapes {
            let shape = self.model.shape(s.id.0);
            let lit = shape
                .and_then(|sm| sm.shader)
                .and_then(|b| self.model.lighting.get(&b))
                .is_some_and(|l| l.texture_set.is_some());
            if !lit {
                rows.push((s.id, s.material.clone(), vec![]));
                continue;
            }
            if shape.is_some_and(|sm| pg_ignore(&self.model, sm)) {
                rows.push((s.id, s.material.clone(), vec![PG_IGNORED.to_string()]));
                continue;
            }
            // B-P41: outside the cache, whose key (the material) does not change when a controller is added.
            let env = shape
                .and_then(|sm| env_scale_ctrl(&self.model, sm))
                .is_some()
                && s.material.slsf2 & SLSF2_TRUE_PBR == 0;
            let c = self.sim_cache.entry(s.id).or_default();
            let hit = c
                .out
                .as_ref()
                .filter(|(m, g, r, _)| *m == s.material && *g == gen && *r == rev);
            let (m, mut notes) = match hit {
                Some((_, _, _, out)) => out.clone(),
                None => {
                    let cands = match &c.cands {
                        Some((slots, g, v)) if *slots == s.material.slots && *g == gen => v.clone(),
                        _ => {
                            let (v, tested) = candidates(files, &nif_path, &s.material.slots);
                            self.match_tests += tested;
                            c.cands = Some((s.material.slots.clone(), gen, v.clone()));
                            v
                        }
                    };
                    let out = simulate_shape(files, &cands, &nif_path, &s.material, exists);
                    c.out = Some((s.material.clone(), gen, rev, out.clone()));
                    self.sim_runs += 1;
                    out
                }
            };
            if env && m.slsf2 & SLSF2_TRUE_PBR != 0 {
                notes.push(PGP_ENV_SCALE.to_string());
            }
            rows.push((s.id, m, notes));
        }
        self.refresh_pg_warnings();
        self.refresh_problems();
        rows
    }

    /// In game before the JSON lands, or when it could not be read: every shape as edited, no line.
    pub fn plain_rows(&self) -> Vec<(ShapeId, MaterialState, Vec<String>)> {
        self.parsed
            .shapes
            .iter()
            .map(|s| (s.id, s.material.clone(), vec![]))
            .collect()
    }

    pub fn materials(&self) -> Vec<(ShapeId, MaterialState)> {
        self.preview_nif()
            .shapes
            .into_iter()
            .map(|s| (s.id, s.material))
            .collect()
    }

    /// Switches what the preview animates (B7's clip selector) and rebuilds the track cache.
    pub fn set_playing(&mut self, p: Playing) {
        self.playing = p;
        self.playing_name = match p {
            Playing::Clip(b) => self
                .model
                .sequences
                .iter()
                .find(|s| s.block == b)
                .and_then(|s| s.name.as_deref().map(str::to_string)),
            Playing::Free => None,
        };
        self.anim = anim_tracks(&self.model, &self.patched, &self.parsed, p);
        self.anim_shown = None;
    }

    /// The Time slider's span in scene time (B-P33): from 0 to the longest cycle, `span / |freq|`, where a Reverse
    /// cycle counts both legs. `ctrl_time` still does each controller's own wrapping, phase included. A clip uses
    /// its sequence's timing. None when nothing is animated, a controller's times are not finite, or the span is zero
    /// (C-P34 (b): only held values, no clock; `tick_anim` still shows them).
    pub fn anim_range(&self) -> Option<(f32, f32)> {
        self.anim_end()
            .filter(|e| e.is_finite() && *e > 0.0)
            .map(|e| (0.0, e))
    }

    /// A controller's times are not finite (+inf, NaN): playback is off and the panel says so.
    pub fn anim_invalid(&self) -> bool {
        self.anim_end().is_some_and(|e| !e.is_finite())
    }

    /// The longest cycle in scene time, NaN or inf when a contributing controller's times are not finite.
    fn anim_end(&self) -> Option<f32> {
        if self.anim.is_empty() {
            return None;
        }
        // freq 0 or a span <= 0 contributes nothing; a NaN span or freq passes through to the result.
        let cycle = |start: f32, stop: f32, freq: f32, reverse: bool| {
            let span = (stop - start) * if reverse { 2.0 } else { 1.0 };
            if freq == 0.0 || span <= 0.0 {
                0.0
            } else {
                span / freq.abs()
            }
        };
        let ends: Vec<f32> = match self.playing {
            Playing::Clip(seq) => {
                let s = self.model.sequences.iter().find(|s| s.block == seq)?;
                vec![cycle(s.start, s.stop, s.frequency, s.cycle == 1)]
            }
            Playing::Free => self
                .anim
                .iter()
                .flat_map(|(_, _, t)| t)
                .map(|t| cycle(t.start, t.stop, t.freq, (t.flags >> 1) & 3 == 1))
                .collect(),
        };
        // f32::max would drop a NaN: keep it, so the range is rejected.
        Some(ends.into_iter().fold(0.0, |a: f32, e| {
            if a.is_nan() || e.is_nan() {
                f32::NAN
            } else {
                a.max(e)
            }
        }))
    }

    /// Each animated shape's material at clock time `t`: `apply_anim` over the cached tracks, nothing parsed.
    pub fn frame_materials(&self, t: f32) -> Vec<(ShapeId, MaterialState)> {
        self.anim
            .iter()
            .map(|(id, base, tracks)| {
                let mut m = apply_anim(base, tracks, t);
                // Solo hides every other shape (preview_nif): a tick must not show it again.
                m.hidden |= self.solo && self.selection != Selection::Shape(id.0);
                (*id, m)
            })
            .collect()
    }
}

/// The tracks of every shape in `p` (its shader's and its alpha property's), for `playing`; shapes with none are
/// left out.
fn anim_tracks(
    m: &NifModel,
    b: &[u8],
    p: &ParsedNif,
    playing: Playing,
) -> Vec<(ShapeId, MaterialState, Vec<Track>)> {
    // B-P41: one header parse, not one per property.
    let Ok(h) = nif::header::Header::parse(b) else {
        return vec![];
    };
    p.shapes
        .iter()
        .filter_map(|s| {
            let sm = m.shape(s.id.0)?;
            let tracks: Vec<Track> = [sm.shader, sm.alpha]
                .into_iter()
                .flatten()
                .flat_map(|prop| match playing {
                    Playing::Free => free_tracks_in(m, &h, b, prop),
                    Playing::Clip(seq) => clip_tracks_in(m, &h, b, seq, prop),
                })
                .collect();
            (!tracks.is_empty()).then(|| (s.id, s.material.clone(), tracks))
        })
        .collect()
}

/// The shape cards of `bytes`, as the scan store would give them for these bytes (`modscan` `write_shapes` and
/// `shapes_of`): the edit-mode cards then show the edited values (Note CZ1). Empty when the bytes do not scan.
pub fn shape_cards(bytes: &[u8]) -> Vec<ShapeView> {
    use modscan::store::{RawSlot, MAX_SLOTS};
    use nif::path::TexPath;
    use nif::scan::Shader;
    let Ok(scan) = nif::scan::scan(bytes) else {
        // Silent on purpose (review Minor 4): the cards fall back to titles, and the same failure blocks Export NIF
        // with its reason, since `read_back` scans the patched bytes too.
        return vec![];
    };
    // modscan's `keep_raw`: the raw string says more than the canonical path.
    let keep_raw = |p: &TexPath| {
        let f = &p.flags;
        f.rebased
            || f.prefixed
            || f.non_dds
            || p.raw.replace('\\', "/").to_ascii_lowercase() != p.norm
    };
    let json = |v: serde_json::Result<String>| v.ok();
    scan.shapes
        .iter()
        .map(|s| {
            let (shader_kind, shader_type, slsf1, slsf2, slots, params): (
                _,
                _,
                _,
                _,
                Vec<&TexPath>,
                _,
            ) = match &s.shader {
                Shader::None => (0, None, None, None, vec![], None),
                Shader::Lighting(l, ts) => (
                    1,
                    Some(l.shader_type),
                    Some(l.slsf1),
                    Some(l.slsf2),
                    ts.iter().take(MAX_SLOTS).collect(),
                    json(serde_json::to_string(&l.params)),
                ),
                Shader::Effect(e) => (
                    2,
                    None,
                    Some(e.slsf1),
                    Some(e.slsf2),
                    vec![&e.source, &e.greyscale],
                    json(serde_json::to_string(&e.params)),
                ),
                Shader::Other { kind } => (
                    3,
                    None,
                    None,
                    None,
                    vec![],
                    Some(serde_json::json!({ "shader": kind }).to_string()),
                ),
            };
            let h = &s.head;
            ShapeView {
                block: s.block,
                name: h.name.clone(),
                kind: format!("{:?}", h.kind),
                shader_kind,
                shader_type,
                slsf1,
                slsf2,
                textures: slots
                    .iter()
                    .map(|p| (!(p.flags.empty || p.norm.is_empty())).then(|| p.norm.clone()))
                    .collect(),
                raw: slots
                    .iter()
                    .map(|p| {
                        keep_raw(p).then(|| RawSlot {
                            raw: p.raw.clone(),
                            flags: p.flags.clone(),
                        })
                    })
                    .collect(),
                alpha_flags: s.alpha.map(|a| a.flags),
                alpha_threshold: s.alpha.map(|a| a.threshold),
                params,
                skinned: h.skinned,
                vertex_desc: h.vertex_desc,
                num_vertices: h.num_vertices,
                num_triangles: h.num_triangles,
                index3d: scan.index3d.get(&s.block).copied(),
            }
        })
        .collect()
}

/// Meshes UI T5b (Note CZ19): the heel height shape `shape` needs, Z = −(lowest bind-pose vertex Z) in root space,
/// rounded to 0.1. A skinned shape's vertices go through the inverse of its NiSkinData skin transform; an unskinned
/// one through its node chain. None when the shape has no drawable geometry (a non-lighting shape, an LE file).
pub fn heel_z_of(bytes: &[u8], shape: u32) -> Option<f32> {
    let parsed = render3d::parse(bytes);
    let s = parsed.shapes.iter().find(|s| s.id.0 == shape)?;
    let h = nif::header::Header::parse(bytes).ok()?;
    let m = nif::geom::skin_to_root(&h, bytes, shape as usize)
        .ok()?
        .unwrap_or(s.world);
    let low = s
        .mesh
        .positions
        .iter()
        .map(|p| m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3])
        .fold(f32::INFINITY, f32::min);
    let z = (-low * 10.0).round() / 10.0;
    z.is_finite().then_some(z + 0.0) // + 0.0: never "-0"
}

/// The other weight file of `data_path`: `…_0.nif` ↔ `…_1.nif` (any case, the case kept). None for other names.
pub fn weight_sibling_of(data_path: &str) -> Option<String> {
    let dot = data_path.len().checked_sub(4)?;
    if !data_path.get(dot..)?.eq_ignore_ascii_case(".nif") {
        return None;
    }
    let (stem, ext) = data_path.split_at(dot);
    let other = match stem.get(stem.len().checked_sub(2)?..)? {
        "_0" => "_1",
        "_1" => "_0",
        _ => return None,
    };
    Some(format!("{}{other}{ext}", &stem[..stem.len() - 2]))
}

/// The Z of the last "NPC" entry of an SDTA string (`[{"name":"NPC","pos":[x,y,Z]}, …]`): the entry RaceMenu uses.
pub fn sdta_npc_z(sdta: &str) -> Option<f32> {
    let v: serde_json::Value = serde_json::from_str(sdta).ok()?;
    let npc = v
        .as_array()?
        .iter()
        .rev()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("NPC"))?;
    npc.get("pos")?.get(2)?.as_f64().map(|z| z as f32)
}

/// `sdta` with its last "NPC" entry's Z set to `z`, other entries kept; an "NPC" entry is appended when there is
/// none. None when the string is not a JSON array.
pub fn sdta_with_npc_z(sdta: &str, z: f32) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(sdta).ok()?;
    let list = v.as_array_mut()?;
    // T5b review Important 2: `z` as given, in its shortest f32 form ("2.25", "7.3"), so it reads back as the same
    // f32 that HH_OFFSET holds.
    let zv: serde_json::Value = serde_json::from_str(&z.to_string()).ok()?;
    let npc = list
        .iter_mut()
        .rev()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("NPC"));
    match npc {
        Some(e) => {
            let pos = e
                .as_object_mut()?
                .entry("pos")
                .or_insert(serde_json::json!([0, 0, 0]));
            match pos.as_array_mut() {
                Some(p) if p.len() >= 3 => p[2] = zv,
                _ => *pos = serde_json::json!([0, 0, zv]),
            }
        }
        None => list.push(serde_json::json!({"name": "NPC", "pos": [0, 0, zv]})),
    }
    serde_json::to_string(&v).ok()
}

/// Meshes UI T2: per shape, "N: title, title (clip)" over its shader's and its alpha's controllers; "none" without.
/// A shader that is not a BSLightingShaderProperty has controllers the editor does not read: its shape lists only its
/// alpha controllers, and gets no line (rather than a wrong "none") when it has none.
pub(crate) fn card_ctrls(m: &NifModel) -> HashMap<u32, String> {
    m.shapes
        .iter()
        .filter_map(|s| {
            let other = s.shader.is_some_and(|b| !m.lighting.contains_key(&b));
            let lit = s.shader.and_then(|b| m.lighting.get(&b));
            let slsf2 = lit.map_or(0, |l| l.lighting.slsf2);
            let props = [
                lit.map(|l| &l.prop),
                s.alpha.and_then(|b| m.alphas.get(&b)).map(|a| &a.prop),
            ];
            let titles: Vec<String> = props
                .into_iter()
                .flatten()
                .flat_map(|p| &p.ctrls)
                .filter_map(|c| m.ctrls.get(c).map(|t| (c, t)))
                .map(|(c, t)| {
                    let title = nif::edit::validate::ctrl_title(t, slsf2);
                    match m.managed(*c) {
                        true => format!("{title} (clip)"),
                        false => title,
                    }
                })
                .collect();
            let line = match titles.len() {
                0 if other => return None,
                0 => "none".to_string(),
                n => format!("{n}: {}", titles.join(", ")),
            };
            Some((s.block, line))
        })
        .collect()
}

/// Meshes UI T2: where Edit opens. The card picked outside edit mode (same block and name, else the only shape with
/// that name, since the shown copy may be PGPatcher's and numbered differently), else the only shape, else Root.
pub fn first_selection(m: &NifModel, picked: Option<&(u32, Option<String>)>) -> Selection {
    if let Some((block, name)) = picked {
        let same = m
            .shapes
            .iter()
            .find(|s| s.block == *block && s.name.as_deref() == name.as_deref());
        let mut named = m
            .shapes
            .iter()
            .filter(|s| name.is_some() && s.name.as_deref() == name.as_deref());
        let by_name = match (named.next(), named.next()) {
            (Some(s), None) => Some(s),
            _ => None,
        };
        if let Some(s) = same.or(by_name) {
            return Selection::Shape(s.block);
        }
    }
    match m.shapes.as_slice() {
        [only] => Selection::Shape(only.block),
        _ => Selection::Root,
    }
}

fn read_source(source: &[u8]) -> Result<(nif::scan::NifScan, NifModel), String> {
    let cannot = |e: nif::NifError| format!("This NIF cannot be edited: {e}");
    let scan = nif::scan::scan(source).map_err(cannot)?;
    Ok((scan, read_model(source).map_err(cannot)?))
}

fn source_id(copy: &PathCopy, data_path: &str, source: &[u8]) -> SourceId {
    SourceId {
        origin: copy.origin.clone(),
        data_path: data_path.into(),
        archive: (copy.kind == KIND_BSA).then(|| copy.root.clone()),
        size: source.len() as u64,
        sha256: edit_doc::sha256_hex(source),
    }
}

/// Each shape's (shader type, slot paths) for [`texture_sources`].
fn slot_paths(p: &ParsedNif) -> Vec<(u32, Vec<String>)> {
    p.shapes
        .iter()
        .map(|s| {
            let m = &s.material;
            (
                m.shader_type,
                m.slots.iter().map(|t| t.norm.clone()).collect(),
            )
        })
        .collect()
}

/// The same folder, ignoring ASCII case, `/` versus `\` and a trailing separator (MO2 paths on Windows).
fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', "\\");
        s.trim_end_matches('\\').to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// Lowercase hex of a SHA-256, as `edit_doc::sha256_hex` writes it.
fn hex(h: [u8; 32]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// The header's block count; 0 when it does not parse.
fn block_count(b: &[u8]) -> u32 {
    nif::header::Header::parse(b).map_or(0, |h| h.sizes.len() as u32)
}

/// A new `Content::Parsed` key: every load of edited bytes is a new load (R10).
fn next_key() -> u64 {
    static KEY: AtomicU64 = AtomicU64::new(1);
    KEY.fetch_add(1, Ordering::Relaxed)
}

impl MeshesModel {
    /// Where NIF edits go: the chosen export folder, and nowhere without one (Note GD5 supersedes CI29's
    /// `<instance>\mods\sdc NIF Edits` default).
    pub fn edit_mod(&self) -> Option<PathBuf> {
        self.nif_export_root.clone()
    }

    /// Note GD5 (c): where an edit of `path` autosaves while no export folder is chosen (beside the scan store).
    fn autosave_of(&self, path: &str) -> Option<PathBuf> {
        let dir = self.store_path.as_ref()?.parent()?;
        edit_doc::autosave_path(dir, path).ok()
    }

    /// Opens an edit session for the selected NIF and shows its patched bytes as `Content::Parsed` (an LE NIF
    /// opens with no preview).
    pub fn open_edit(&mut self) {
        self.drop_edit();
        // Final review Minor 8: the column before Edit (Edit locks it on Shapes); `drop_edit` puts it back.
        let column = self.shapes_col;
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        let edit_mod = self.edit_mod();
        match self.start_edit(&row.path, edit_mod.as_deref()) {
            Ok((mut e, content)) => {
                e.selection = first_selection(&e.model, self.card_sel.as_ref());
                self.edit = Some(e);
                self.shapes_col_before = Some(column);
                // The edit opens on Edited source (v2 B8: In game is pgpsim over it).
                self.view_source = true;
                if let Some(c) = content {
                    self.v3.set_content(Some(c), false);
                    self.v3.nif_view = true;
                }
                self.refresh_placement();
                self.edit_checks();
            }
            Err(e) => {
                // A previous session's Parsed preview would no longer match the error.
                if !self.v3.is_mesh() {
                    self.load_shapes();
                }
                self.error = Some(e);
            }
        }
    }

    fn start_edit(
        &self,
        path: &str,
        edit_mod: Option<&Path>,
    ) -> Result<(NifEdit, Option<Content>), String> {
        let store = self.store.as_ref().ok_or("No scan store.")?;
        edit_doc::win(path)?; // before the read: a loose copy's root joined with `..` would read outside its mod
        let copies = store.copies_of_path(path).map_err(db_err)?;
        let src = pick_source(&copies, edit_mod.unwrap_or(Path::new("")))
            .ok_or_else(|| format!("No source copy of {path} to edit."))?
            .clone();
        let bytes = read_file(&FileSource::from_store(src.kind, &src.root, path))?;
        let autosave = self.autosave_of(path);
        let mut e = NifEdit::open(bytes, src, path, edit_mod, autosave.as_deref())?;
        e.copies = copies;
        e.weight_sibling = weight_sibling_of(path)
            .filter(|s| store.copies_of_path(s).is_ok_and(|c| !c.is_empty()));
        let c = self.edit_content(&e)?;
        Ok((e, c))
    }

    /// The session's patched bytes as a new `Content::Parsed` (None for an LE NIF).
    fn edit_content(&self, e: &NifEdit) -> Result<Option<Content>, String> {
        let store = self.store.as_ref().ok_or("No scan store.")?;
        if e.le_mesh {
            return Ok(None);
        }
        let mut nif = e.preview_nif();
        nif.head_part = store.is_head_part_mesh(&e.data_path).unwrap_or(false); // shading only, as mesh_spec
        let textures = texture_sources(store, &slot_paths(&nif)).map_err(db_err)?;
        Ok(Some(Content::Parsed {
            key: next_key(),
            nif: Arc::new(nif),
            textures: Arc::new(textures),
        }))
    }

    /// Shows the open session's edit preview, rebuilt from its current edits, on the toggle's side (v2 B8: In game
    /// overlays the simulated materials).
    /// False when there is none to show (no session, an LE NIF, or an error, which goes to `error`).
    pub(super) fn show_edit_preview(&mut self) -> bool {
        self.refresh_in_game();
        let Some(e) = &self.edit else { return false };
        match self.edit_content(e) {
            Ok(Some(c)) => {
                self.v3.set_content(Some(c), false);
                self.v3.nif_view = true;
                if let Some(e) = &mut self.edit {
                    e.anim_shown = None;
                }
                true
            }
            Ok(None) => false,
            Err(err) => {
                self.error = Some(err);
                false
            }
        }
    }

    /// An edit is open but the 3D view shows a plain copy (`show_edit_preview` failed): the edits are not on screen.
    pub fn edit_preview_hidden(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| !e.le_mesh) && !self.v3.nif_view
    }

    /// Ends the session (an unsaved drag is saved) and shows the NIF as a mesh again.
    pub fn close_edit(&mut self) {
        if self.edit.is_none() {
            return;
        }
        self.drop_edit();
        self.load_shapes();
    }

    /// Ends the session without touching the view's content.
    pub(super) fn drop_edit(&mut self) {
        if let Some(column) = self.shapes_col_before.take() {
            self.shapes_col = column;
        }
        if let Some(mut e) = self.edit.take() {
            e.flush();
            if let Some(err) = e.error {
                self.error = Some(err);
            }
        }
        self.v3.nif_view = false;
        self.v3.clock = None;
    }

    /// One drawn frame of the animation (v2 B3). Call once per frame the Meshes 3D view is drawn, before
    /// `ui::viewport3d::show`, so that frame's Render carries the new time. Keeps the clock in step with the
    /// session's tracks, advances it, and sends each animated shape's material (cbuffer only) when the time moved.
    pub fn tick_anim(&mut self, dt: f32) {
        // A shape animated last frame that no longer is (a removed controller, B7's clip switch) shows its file
        // material again: nothing else resends it, its static material did not change.
        if let Some(e) = self.edit.as_mut() {
            let now: Vec<ShapeId> = e.anim.iter().map(|(id, _, _)| *id).collect();
            if e.anim_last.iter().any(|id| !now.contains(id)) {
                let gone: Vec<_> = e
                    .materials() // clones the parse (F22): only on the frame a shape leaves the set
                    .into_iter()
                    .filter(|(id, _)| e.anim_last.contains(id) && !now.contains(id))
                    .collect();
                for (id, m) in gone {
                    // false: no load sent yet; the next load carries the file material
                    self.v3.set_material(id, m);
                }
            }
            e.anim_last = now;
        }
        let in_game = self.edit.as_ref().is_some_and(|e| e.in_game.is_some());
        let Some(range) = self.edit.as_ref().and_then(NifEdit::anim_range) else {
            self.v3.clock = None;
            // C-P34 (b): a zero-length range (held values only) has no clock, but its values still show.
            let held = self
                .edit
                .as_ref()
                .is_some_and(|e| !e.anim.is_empty() && !e.anim_invalid());
            if held && self.v3.nif_view && !in_game {
                self.show_anim_frame(0.0);
            }
            return;
        };
        let c = self.v3.clock.get_or_insert(Clock::new(range));
        if c.range != range {
            c.range = range;
            c.t = c.t.clamp(range.0, range.1);
        }
        // In game (simulated) shows static PGPatcher materials (F21): the clock pauses there.
        if !self.v3.nif_view || in_game {
            c.playing = false;
            return;
        }
        self.v3.tick(dt);
        if let Some(c) = self.v3.clock {
            self.show_anim_frame(c.t);
        }
    }

    /// Sends each animated shape's material at time `t`, once per new `t`.
    fn show_anim_frame(&mut self, t: f32) {
        let Some(e) = self.edit.as_mut() else {
            return;
        };
        if e.anim_shown == Some(t) {
            return;
        }
        for (id, m) in e.frame_materials(t) {
            // false: no load of this content sent yet (a reload); the next frame tries again.
            if !self.v3.set_material(id, m) {
                return;
            }
        }
        e.anim_shown = Some(t);
    }

    pub fn edit_push(&mut self, ops: Vec<Op>, merge: Option<String>) {
        let Some(e) = &mut self.edit else { return };
        let before = e.materials();
        e.push(ops, merge);
        self.edit_changed(before);
    }

    pub fn undo_edit(&mut self) -> bool {
        let Some(e) = &mut self.edit else {
            return false;
        };
        let before = e.materials();
        let done = e.undo();
        self.edit_changed(before);
        done
    }

    pub fn redo_edit(&mut self) -> bool {
        let Some(e) = &mut self.edit else {
            return false;
        };
        let before = e.materials();
        let done = e.redo();
        self.edit_changed(before);
        done
    }

    pub fn select_edit(&mut self, s: Selection) {
        let Some(e) = &mut self.edit else { return };
        let before = e.materials();
        e.selection = s;
        self.edit_changed(before);
    }

    /// Runs `f` on the session (Solo, Keep mine, Take source, Revert) and updates the preview.
    pub fn edit_with(&mut self, f: impl FnOnce(&mut NifEdit)) {
        let Some(e) = &mut self.edit else { return };
        let before = e.materials();
        f(e);
        self.edit_changed(before);
    }

    /// "Check that files exist": on, the slot warnings include missing files; off, they are dropped again.
    pub fn set_edit_check_exists(&mut self, on: bool) {
        let Some(e) = &mut self.edit else { return };
        e.check_exists = on;
        e.warnings = validate(&e.model, None);
        demote_source(&e.source_warnings, &mut e.warnings);
        self.edit_checks();
    }

    /// The Browse list: scanned winning texture paths that contain `filter` (at most `limit`).
    pub fn texture_paths(&self, filter: &str, limit: usize) -> Vec<String> {
        self.store
            .as_ref()
            .and_then(|s| s.texture_paths(filter, limit).ok())
            .unwrap_or_default()
    }

    /// The session's error, the missing-file check when it is on, the export block and the session's NIF badge.
    fn edit_checks(&mut self) {
        let Some(e) = &mut self.edit else { return };
        if let Some(err) = e.error.take() {
            self.error = Some(err);
        }
        if let (true, Some(store)) = (e.check_exists, &self.store) {
            let exists = |n: &str| matches!(store.texture_location(n), Ok(Some(_)));
            e.warnings = validate(&e.model, Some(&exists));
            demote_source(&e.source_warnings, &mut e.warnings);
        }
        e.refresh_blocked();
        if !e.edit_mod.as_os_str().is_empty() {
            match DocBadge::of(&e.doc) {
                Some(b) => self.doc_badges.insert(e.data_path.clone(), b),
                None => self.doc_badges.remove(&e.data_path),
            };
        }
    }

    /// Export steps 1-7 for the open session (spec §8): the outcome also sets the session's guard and result line.
    pub fn export_edit(&mut self, choice: Option<GuardChoice>) -> ExportOutcome {
        let Some(e) = &mut self.edit else {
            return ExportOutcome::Blocked("No NIF is being edited.".into());
        };
        let before = e.materials();
        let out = e.export(choice);
        self.after_export(before, &out);
        out
    }

    /// The open session's result line after an export, and the preview (a changed source re-applied the ops).
    fn after_export(&mut self, before: Vec<(ShapeId, MaterialState)>, out: &ExportOutcome) {
        self.foreign_cache.take();
        let Some(e) = &mut self.edit else { return };
        e.export_note = match out {
            ExportOutcome::Written { rerun_pgpatcher } => Some((
                match rerun_pgpatcher {
                    true => RERUN_PGPATCHER.into(),
                    false => "Exported.".into(),
                },
                false,
            )),
            ExportOutcome::Blocked(s) => Some((s.clone(), true)),
            ExportOutcome::KeptTheirs => {
                Some(("Not exported: the file on disk was kept.".into(), false))
            }
            ExportOutcome::Guard { .. } => None,
        };
        // A changed source re-applied the ops: the preview follows.
        self.edit_changed(before);
    }

    /// Note GD2: one row per NIF with pending edits (today the open edit only), for the Review. Reads the export
    /// path's state from the disk only when its length, time or the last export's hash changed.
    pub fn nif_review_rows(&self) -> Vec<NifReviewRow> {
        let Some(e) = self.edit.as_ref().filter(|e| e.doc.pending()) else {
            return vec![];
        };
        let mut blocked = e.blocked.clone();
        let foreign = match e.export_path.as_os_str().is_empty() {
            true => None,
            false => self.disk_state(e).unwrap_or_else(|why| {
                blocked.get_or_insert(why);
                None
            }),
        };
        let mut answer = None;
        if let (Some(f), Some((c, h))) = (
            foreign,
            self.nif_answers.get(&e.data_path.to_ascii_lowercase()),
        ) {
            match *h == f {
                true => answer = Some(*c),
                false => {
                    blocked.get_or_insert(NIF_CHANGED_SINCE.into());
                }
            }
        }
        vec![NifReviewRow {
            path: e.data_path.clone(),
            mod_name: e
                .edit_mod
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            blocked,
            foreign,
            answer,
            compaction: e.compaction,
        }]
    }

    /// The export path of `e` as the export would find it: None when absent or Tessalith's own, Some(SHA-256) when
    /// Tessalith did not write it, Err when it cannot be read.
    // shortcut: the key leaves out the edit's bytes, so a file that equals them reads as foreign until it changes;
    // add `e.rev` if that case shows up (it would re-read the file on every drag tick).
    fn disk_state(&self, e: &NifEdit) -> Result<Option<[u8; 32]>, String> {
        let meta = std::fs::metadata(&e.export_path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        let key = (e.export_path.clone(), e.doc.export.sha256.clone(), meta);
        if let Some((k, v)) = &*self.foreign_cache.borrow() {
            if *k == key {
                return v.clone();
            }
        }
        let cannot =
            |err: std::io::Error| format!("Cannot read {}: {err}", e.export_path.display());
        let last = e.doc.export.sha256.clone();
        let ours = |b: &[u8]| last.as_deref() == Some(edit_doc::sha256_hex(b).as_str());
        let v = match engine::dds::may_replace(&e.export_path, &e.out, ours) {
            Ok(()) => Ok(None),
            Err(Kept::Unreadable(err)) => Err(cannot(err)),
            Err(Kept::Foreign) => std::fs::read(&e.export_path)
                .map(|b| Some(Sha256::digest(&b).into()))
                .map_err(cannot),
        };
        *self.foreign_cache.borrow_mut() = Some((key, v.clone()));
        v
    }

    /// Note GD2: Review's answer for the file at `path`'s export path, given for the file with hash `sha256`. It
    /// counts only while the file still has that hash, and for one export.
    pub fn set_nif_answer(&mut self, path: &str, choice: GuardChoice, sha256: [u8; 32]) {
        self.nif_answers
            .insert(path.to_ascii_lowercase(), (choice, sha256));
    }

    /// Note GD2: writes every Review row that is not blocked and, when Tessalith did not write its file, is answered
    /// Overwrite for the bytes on disk now (read again here). The answers are used up. Ok(written) when nothing was
    /// skipped, else Err listing each skipped NIF and why. Each write is a line in Messages, with its placement.
    pub fn export_nif_edits(&mut self) -> Result<usize, String> {
        self.foreign_cache.take(); // check again at write time
        let rows = self.nif_review_rows();
        self.nif_answers.clear();
        let (mut written, mut skipped) = (0, vec![]);
        for r in rows {
            let why = match (&r.blocked, r.foreign, r.answer) {
                (Some(b), _, _) => Some(b.clone()),
                (None, Some(_), None) => Some(NIF_UNANSWERED.to_string()),
                (None, Some(_), Some(GuardChoice::KeepTheirs)) => Some(NIF_KEPT.to_string()),
                _ => None,
            };
            if let Some(why) = why {
                skipped.push(format!("{}: {why}", r.path));
                continue;
            }
            let Some(e) = self.edit.as_mut().filter(|e| e.data_path == r.path) else {
                continue;
            };
            let before = e.materials();
            let out = e.write_out(r.foreign.map(hex));
            e.guard = None; // the Review answers on its row: no dialog
            let placement = e.placement.clone();
            self.after_export(before, &out);
            match out {
                ExportOutcome::Written { .. } => {
                    written += 1;
                    self.messages
                        .push(format!("Exported {} to {}.", r.path, r.mod_name));
                    if let Some(p) = placement.filter(|p| *p != Placement::Ok) {
                        self.messages.push(p.text(&r.mod_name));
                    }
                }
                ExportOutcome::Blocked(why) => skipped.push(format!("{}: {why}", r.path)),
                ExportOutcome::Guard { .. } | ExportOutcome::KeptTheirs => {
                    skipped.push(format!("{}: {NIF_CHANGED_SINCE}", r.path))
                }
            }
        }
        match skipped.is_empty() {
            true => Ok(written),
            false => Err(format!("NIF edits skipped: {}", skipped.join("; "))),
        }
    }

    /// Note GD1: the open edit's worklist rows; the same `Arc` until they change (an empty one without an edit).
    pub fn nif_problems(&self) -> Arc<Vec<NifProblem>> {
        match &self.edit {
            Some(e) => e.problems.clone(),
            None => self.no_problems.clone(),
        }
    }

    /// A worklist row's fix (Note GD1): opens the NIF at `path` when another one (or none) is open, then asks the edit
    /// panel for `tab` and, when it draws it, focus on `field`.
    pub fn show_nif_tab(&mut self, path: &str, tab: NifTab, field: Option<Field>) {
        let open = self
            .edit
            .as_ref()
            .is_some_and(|e| e.data_path.eq_ignore_ascii_case(path));
        if !open {
            let row = self
                .winners
                .iter()
                .map(|(n, _)| n)
                .find(|n| n.path.eq_ignore_ascii_case(path))
                .map(|n| n.copy_id);
            let Some(id) = row else {
                self.error = Some(format!("{path} is not in the scanned meshes."));
                return;
            };
            self.select_nif(id);
            self.open_edit();
        }
        if self.edit.is_some() {
            self.nif_tab = Some((tab, field));
        }
    }

    /// The badge of every edit document in the edit mod, keyed by data path (walks `.sdc\nif-edits`).
    pub fn pending_docs(&self) -> HashMap<String, DocBadge> {
        self.edit_mod()
            .map(|m| super::export::pending_docs(&m).into_iter().collect())
            .unwrap_or_default()
    }

    /// `doc_badges` from disk; after a scan, or a change of instance.
    pub(super) fn refresh_doc_badges(&mut self) {
        self.doc_badges = self.pending_docs();
    }

    /// The session's placement line: from the store's copies and mod rows, and the modlist to tell a mod that is not
    /// scanned yet from one that is not enabled.
    pub(super) fn refresh_placement(&mut self) {
        let Some(e) = &mut self.edit else { return };
        let (Some(ScanTarget::Mo2 { root, profile }), false) =
            (&self.target, e.edit_mod.as_os_str().is_empty())
        else {
            e.placement = None;
            return;
        };
        let inst = Instance::load(root, Some(profile)).ok();
        // Only a folder directly in the instance's `mods` is an MO2 mod (Note CI29); its folder name is the mod name.
        let name = match (e.edit_mod.parent(), e.edit_mod.file_name(), &inst) {
            (Some(parent), Some(name), Some(i)) if same_dir(parent, &i.mods) => {
                name.to_string_lossy().into_owned()
            }
            _ => {
                e.placement = Some(Placement::NotInMo2);
                return;
            }
        };
        let mut p = placement(&e.copies, &self.mods, &name);
        if p == Placement::NotEnabled {
            let listed = inst
                .and_then(|i| modscan::mo2::loose_origins(&i).ok())
                .is_some_and(|(on, missing)| {
                    on.iter().any(|o| o.name.eq_ignore_ascii_case(&name))
                        || missing.iter().any(|n| n.eq_ignore_ascii_case(&name))
                });
            if listed {
                p = Placement::NotScanned;
            }
        }
        e.placement = Some(p);
    }

    /// The app sets the Textures export folder each frame (Note CI29). A change re-reads the chips. An open session
    /// with ops keeps them in memory and moves to the new folder unsaved (`NifEdit::retarget`, Note GD5); one with no
    /// ops has nothing live, so the same NIF is reopened from the new folder: its edit document there, or a fresh one.
    /// The selected shape is kept.
    pub fn set_nif_export_root(&mut self, root: Option<PathBuf>) {
        if self.nif_export_root == root {
            return;
        }
        self.nif_export_root = root;
        self.refresh_doc_badges();
        let Some(selection) = self.edit.as_ref().map(|e| e.selection) else {
            return;
        };
        if let Some(e) = self.edit.as_mut().filter(|e| !e.doc.ops.is_empty()) {
            match e.retarget(self.nif_export_root.as_deref()) {
                Ok(lines) => self.messages.extend(lines),
                Err(err) => self.error = Some(err),
            }
            self.refresh_placement();
            self.edit_checks();
            return;
        }
        // Flushes to the old folder.
        self.drop_edit();
        // The selected NIF is the edited one: selecting another ends the session.
        self.open_edit();
        // Root too (T2 review Minor 1): the reopen starts on `first_selection`, which may be a shape.
        let keep = self.edit.as_ref().is_some_and(|e| match selection {
            Selection::Root => true,
            Selection::Shape(id) => e.parsed.shapes.iter().any(|s| s.id.0 == id),
        });
        if keep {
            self.select_edit(selection);
        }
    }

    /// The preview after a change (R10): each changed shape's material goes to the loaded content; a texture path
    /// the content's map lacks (and the store has), or a material the viewport refuses, reloads the same view with
    /// the map extended.
    pub(super) fn edit_changed(&mut self, before: Vec<(ShapeId, MaterialState)>) {
        // It sends static materials: the next tick_anim resends the animated ones.
        if let Some(e) = &mut self.edit {
            e.anim_shown = None;
        }
        self.edit_checks();
        self.refresh_in_game();
        let (Some(e), Some(store)) = (&self.edit, &self.store) else {
            return;
        };
        if e.le_mesh {
            return;
        }
        // A failed show_edit_preview left a plain copy on screen: the session is current, and the next switch
        // rebuilds the preview from it.
        let Some(Content::Parsed { nif, textures, .. }) = self.v3.content() else {
            return;
        };
        let (head_part, textures) = (nif.head_part, textures.clone());
        let mut next = e.preview_nif();
        next.head_part = head_part;
        let fresh = match texture_sources(store, &slot_paths(&next)) {
            Ok(f) => f,
            Err(err) => {
                self.error = Some(db_err(err));
                return;
            }
        };
        let mut reload = fresh.keys().any(|k| !textures.contains_key(k));
        if !reload {
            // The render thread keeps these edits and re-applies them on a reload it starts itself.
            for s in &next.shapes {
                let old = before.iter().find(|(id, _)| *id == s.id).map(|(_, m)| m);
                if old != Some(&s.material) && !self.v3.set_material(s.id, s.material.clone()) {
                    reload = true;
                    break;
                }
            }
        }
        if reload {
            let mut map = (*textures).clone();
            for (k, v) in fresh {
                map.entry(k).or_insert(v);
            }
            let c = Content::Parsed {
                key: next_key(),
                nif: Arc::new(next),
                textures: Arc::new(map),
            };
            self.v3.set_content(Some(c), true);
        }
    }
}
