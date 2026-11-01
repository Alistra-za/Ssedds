//! The user's texture sets (M3b spec §3, §6.1): views for the Sets column and set editor, shared-slot ownership,
//! the engine inputs (`set_edits`, `output_paths`) and the drop rules. No egui types.
use super::history::{sha256_of, CreatedFile};
use super::items::{ItemKind, ItemStatus, TextureItem};
use super::{AppModel, ViewCache};
use crate::pgp::prefix::{
    active_matches, check_file_name, check_rel_path, derive_match_diffuse, extra_match_refusal,
    is_path_key, output_rel, prefix_of, slot_match_path, slot_suffixes, strip_lead,
};
use crate::pgp::settings::Settings;
use crate::sets_doc::{SetEntry, SetsDoc};
use engine::codes::Warning;
use engine::config::{Subtype, Target};
use engine::group::{base_of, split_vocab, VocabRole};
use engine::overrides::path_key_matches;
use engine::presets::Provenance;
use engine::sets::{fold_id, same_id, set_id, OutputKey, SetEdit};
use engine::shader::{Shader, ShaderPick};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Diffuse,
    Normal,
    Emissive,
    /// The Advanced skin `_sk` (slot 3, shared with the emissive).
    SkinSk,
    Height,
    Rmaos,
    Coat,
    Fuzz,
    Subsurface,
}

impl SlotKind {
    /// Every slot kind, in the slot commands' order. The set editor orders its cards by `slot_row` (F1 spec §5).
    pub const ALL: [SlotKind; 9] = [
        SlotKind::Diffuse,
        SlotKind::Normal,
        SlotKind::Rmaos,
        SlotKind::Height,
        SlotKind::Emissive,
        SlotKind::SkinSk,
        SlotKind::Subsurface,
        SlotKind::Coat,
        SlotKind::Fuzz,
    ];
    /// PGPatcher's 1-based `slotN` (DECISIONS P6).
    pub fn slot_number(self) -> u8 {
        match self {
            SlotKind::Diffuse => 1,
            SlotKind::Normal => 2,
            SlotKind::Emissive | SlotKind::SkinSk => 3,
            SlotKind::Height => 4,
            SlotKind::Rmaos => 6,
            SlotKind::Coat | SlotKind::Fuzz => 7,
            SlotKind::Subsurface => 8,
        }
    }
    /// The canonical file suffix under the prefix (`""` for the diffuse).
    pub fn suffix(self) -> &'static str {
        match self {
            SlotKind::Diffuse => "",
            SlotKind::Normal => "n",
            SlotKind::Emissive => "g",
            SlotKind::SkinSk => "sk",
            SlotKind::Height => "p",
            SlotKind::Rmaos => "rmaos",
            SlotKind::Coat => "cnr",
            SlotKind::Fuzz => "f",
            SlotKind::Subsurface => "s",
        }
    }
    /// The slot's name without the number: `label`, but a shared slot names both kinds (as `slot_label`'s empty card).
    pub fn short_label(self) -> &'static str {
        match self.slot_number() {
            3 => "Emissive / Skin",
            7 => "Coat normal / Fuzz",
            _ => self.label(),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SlotKind::Diffuse => "Diffuse",
            SlotKind::Normal => "Normal _n",
            SlotKind::Emissive => "Emissive _g",
            SlotKind::SkinSk => "Skin _sk",
            SlotKind::Height => "Height _p",
            SlotKind::Rmaos => "RMAOS",
            SlotKind::Coat => "Coat _cnr",
            SlotKind::Fuzz => "Fuzz _f",
            SlotKind::Subsurface => "Subsurface _s",
        }
    }
}

/// The card title of `slot` holding item `key`: a `_coat` source in the Subsurface slot is the coat colour + strength
/// (M6 Track E spec §3.1, DECISIONS Note U2), decided by the file name.
pub fn slot_label(slot: SlotKind, key: &str) -> &'static str {
    let stem = Path::new(key)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    match split_vocab(&stem) {
        Some((_, v)) if slot == SlotKind::Subsurface && v.role == VocabRole::CoatColour => {
            "Coat colour + strength"
        }
        _ if slot.slot_number() == 7 => SLOT7_CARD,
        _ if key.is_empty() && slot.slot_number() == 3 => SLOT3_CARD,
        _ => slot.label(),
    }
}

/// Note AO: the one card of PGPatcher slot 7, which holds the coat normal `_cnr` or the fuzz `_f`.
pub const SLOT7_CARD: &str = "Coat normal / Fuzz (slot 7)";
/// Note AO: the empty card of PGPatcher slot 3, which holds the emissive `_g` or the Advanced skin `_sk`.
pub const SLOT3_CARD: &str = "Emissive / Skin (slot 3)";

/// Note AO: the kinds a card of `slot`'s PGPatcher slot takes (`Coat _cnr or Fuzz _f` for slot 7).
pub fn slot_kinds_label(slot: SlotKind) -> String {
    let kinds: Vec<&str> = SlotKind::ALL
        .iter()
        .filter(|k| k.slot_number() == slot.slot_number())
        .map(|k| k.label())
        .collect();
    kinds.join(" or ")
}

/// Note F?11 (A15): slot `n` as the set editor's empty card names it, without the "(slot n)" its title adds (the Match
/// menu and Slot commands say the number themselves). Slot 5 has no card: the cubemap. A vanilla set (`!pbr`) takes the
/// Meshes tab's name for the texture-set slot (`edit_slot_label`, read only; keep in step with nif_edit.rs).
pub fn slot_name(pbr: bool, n: u8) -> &'static str {
    if !pbr {
        return super::meshes::nif_edit::edit_slot_label(0, 0, 0, usize::from(n.max(1) - 1));
    }
    match SlotKind::ALL.iter().find(|k| k.slot_number() == n) {
        Some(k) => k.short_label(),
        None => "Cubemap",
    }
}

/// The card title of `slot` holding `key` in a set drawn by `shader`: a grass file in the Diffuse card is "Complex grass".
pub fn card_title(slot: SlotKind, key: &str, shader: Option<Shader>) -> &'static str {
    if slot == SlotKind::Diffuse && shader == Some(Shader::ComplexGrass) {
        return "Complex grass";
    }
    slot_label(slot, key)
}

/// One row of the Matching header, or one choice of its + menu (F1 spec §3.2): slot number, slot name, path (stored
/// form, no `.dds`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotMatch {
    pub n: u8,
    pub slot: String,
    pub path: String,
}

/// The Matching header of `v`: its active slot matches plus the empty rows (`matchN = ""`, N ≥ 2: added by + and not
/// filled in yet; `match1 = ""` is the diffuse turned off), and the + menu, which offers slots 3–8 without a row. A
/// slot the set fills starts from `slot_match_path` (DECISIONS V1); any other starts empty (owner, 2026-10-12).
pub fn slot_matches(v: &SetView) -> (Vec<SlotMatch>, Vec<SlotMatch>) {
    let derived = v.derived_match.as_deref().unwrap_or("");
    let filled: Vec<SlotKind> = v
        .slots
        .iter()
        .filter(|(_, f)| !matches!(f, SlotFill::Empty))
        .map(|(k, _)| *k)
        .collect();
    let name = |n: u8| match filled.iter().find(|k| k.slot_number() == n) {
        Some(k) => k.label().to_string(),
        None if n == 1 => SlotKind::Diffuse.label().to_string(),
        None if n == 2 => SlotKind::Normal.label().to_string(),
        None => format!("Slot {n}"),
    };
    let empty = (2..=8u8).filter(|n| {
        v.settings
            .get(&format!("match{n}"))
            .and_then(Value::as_str)
            .is_some_and(|s| s.trim().is_empty())
    });
    let mut active: Vec<SlotMatch> = active_matches(&v.settings, derived)
        .into_iter()
        .chain(empty.map(|n| (n, String::new())))
        .map(|(n, path)| SlotMatch {
            n,
            slot: name(n),
            path,
        })
        .collect();
    active.sort_by_key(|a| a.n);
    let add = (3..=8u8)
        .filter(|n| !active.iter().any(|a| a.n == *n))
        .map(|n| SlotMatch {
            n,
            slot: name(n),
            // Prefilled only when PGPatcher's slot-N list strips the suffix, so its prefix is sdc's (controller
            // ruling): `_s` in slot 8 would split it, so that row starts empty.
            path: filled
                .iter()
                .find(|k| {
                    k.slot_number() == n
                        && slot_suffixes(n).contains(&format!("_{}", k.suffix()).as_str())
                })
                .map_or_else(String::new, |k| slot_match_path(derived, k.suffix())),
        })
        .collect();
    (active, add)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotFill {
    Empty,
    Own {
        item: String,
    },
    /// A finished output of set `from`, used here through a slot command.
    Borrowed {
        from: String,
        item: String,
    },
}

/// A line of an `[[sets]]` entry that matches nothing: the card's red "?" chip and a Problems row (Note DU).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// The entry's files have all gone (its fix is Delete set).
    NoFilesLeft,
    /// An `add` or `remove` path that names no file.
    File(String),
    /// `borrow.slot<slot>` from set `from`, which is gone or (`empty`) has nothing in that slot.
    Borrow { slot: u8, from: String, empty: bool },
}

impl Missing {
    pub fn text(&self) -> String {
        match self {
            Missing::NoFilesLeft => NO_FILES_LEFT.into(),
            Missing::File(f) => f.clone(),
            Missing::Borrow { slot, from, empty } => {
                let none = if *empty { " (it has none)" } else { "" };
                format!("slot{slot} from {from}{none}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetView {
    /// The engine key (`""` for a created set with no members yet).
    pub key: String,
    pub id: String,
    /// Item keys of every member, packed outputs and raw sources included.
    pub members: Vec<String>,
    pub slots: BTreeMap<SlotKind, SlotFill>,
    /// A TruePBR JSON set: the diffuse resolves to `pbr_albedo`. Not `skin_albedo` since AG90 (owner: Advanced skin
    /// sets get no PGPatcher JSON); the engine's PBR follow-ons still use [`is_pbr_diffuse`].
    pub pbr: bool,
    /// The diffuse resolves to `skin_albedo`: an Advanced skin set (no JSON, the skin card, AG90).
    pub skin: bool,
    pub created: bool,
    /// `add`/`remove` entries of `sdc-project.toml` that match no file (Review focus 1).
    pub missing: Vec<Missing>,
    /// Slots whose finished output this set owns while another set borrows it.
    pub owns: Vec<SlotKind>,
    pub derived_match: Option<String>,
    /// The output prefix, or why there is none (shown, and an export block for a PBR set on CS).
    pub prefix: Option<String>,
    pub prefix_error: Option<String>,
    /// The set's own JSON settings.
    pub settings: Settings,
    /// M6: the engine's shader pick (from the plan), the saved choice (from `sdc-project.toml`), and the set's
    /// SHADER-* warnings.
    pub shader: Option<ShaderPick>,
    pub shader_choice: Option<Shader>,
    pub set_warnings: Vec<Warning>,
    /// F3a Task 8: the locked slot numbers (`SetEntry::lock`).
    pub locks: BTreeSet<u8>,
    /// The engine's recipe reports for this set (`SetItem::recipes`), read by the Sets column pill.
    pub recipes: Vec<engine::events::RecipeReport>,
}

impl SetView {
    pub(crate) fn new(key: String, id: String, members: Vec<String>) -> SetView {
        SetView {
            key,
            id,
            members,
            slots: BTreeMap::new(),
            pbr: false,
            skin: false,
            created: false,
            missing: vec![],
            owns: vec![],
            derived_match: None,
            prefix: None,
            prefix_error: None,
            settings: Settings::new(),
            shader: None,
            shader_choice: None,
            set_warnings: vec![],
            locks: BTreeSet::new(),
            recipes: vec![],
        }
    }
}

/// Whether `v` is written as PGPatcher JSON: a PBR set under Community Shaders (the set card badge). An Advanced
/// skin set is not (AG90).
pub fn is_json_set(v: &SetView, target: Target) -> bool {
    v.pbr && target == Target::Cs
}

/// The set's pick is `pbr_grass` (PBR grass design §4.3): its PGPatcher settings are the grass view.
pub fn is_pbr_grass(v: &SetView) -> bool {
    v.shader
        .as_ref()
        .is_some_and(|p| p.shader == Shader::PbrGrass)
}

pub const BADGE_PBR_LANDSCAPE: &str = "PBR Landscape → CS JSON";
pub const BADGE_PBR_LANDSCAPE_PGP: &str = "PBR Landscape → CS + PGPatcher JSON";
pub const BADGE_PBR_LANDSCAPE_PGP_ONLY: &str = "PBR Landscape → PGPatcher JSON (no CS records yet)";
pub const BADGE_PBR_LANDSCAPE_NONE: &str = "PBR Landscape (no JSON yet)";

/// The Sets column badge. A PBR Landscape set's says what it writes (Note BM, review I2): CS records when a block
/// has an EDID, the PGPatcher entry when the set is a PGPatcher JSON set its statics rule keeps.
pub fn badge_text(m: &AppModel, v: &SetView) -> &'static str {
    if v.skin {
        // AG90: an Advanced skin set gets no PGPatcher JSON.
        "Advanced skin (no JSON)"
    } else if is_pbr_landscape(v) {
        match (m.cs_records_written(v), m.pgp_written(v)) {
            (true, true) => BADGE_PBR_LANDSCAPE_PGP,
            (true, false) => BADGE_PBR_LANDSCAPE,
            (false, true) => BADGE_PBR_LANDSCAPE_PGP_ONLY,
            (false, false) => BADGE_PBR_LANDSCAPE_NONE,
        }
    } else if !is_json_set(v, m.target) {
        "vanilla set (no JSON)"
    } else {
        "PBR → JSON"
    }
}

/// Layout v2 A5: the short kind on the set card's pill (`badge_text` is its hover).
pub fn kind_text(m: &AppModel, v: &SetView) -> &'static str {
    if v.skin {
        "skin"
    } else if is_pbr_landscape(v) {
        "PBR Landscape"
    } else if is_json_set(v, m.target) {
        "PBR → JSON"
    } else {
        "vanilla"
    }
}

/// The set's pick is `pbr_landscape` (Note BM): it gets the PBR Landscape card and badge.
pub fn is_pbr_landscape(v: &SetView) -> bool {
    v.shader
        .as_ref()
        .is_some_and(|p| p.shader == Shader::PbrLandscape)
}

/// Whether the set editor draws the TruePBR panel and the JSON path errors: a JSON set, or any set but a skin set
/// that already has JSON settings (so existing edits are never hidden). `pbr_export::export_blocks` checks `v.pbr`.
pub fn shows_json_panel(v: &SetView, target: Target) -> bool {
    is_json_set(v, target) || (!v.skin && !v.settings.is_empty())
}

/// A diffuse sub-type whose members follow as PBR (DECISIONS P5, Note R): `pbr_albedo`, or the Advanced skin
/// `skin_albedo` (final review C1). It no longer puts a skin set in the PGPatcher JSON: `SetView::pbr` does (AG90).
/// Not the engine's `is_pbr_color`, which decides sRGB and leaves `skin_albedo` (UNORM) out.
pub fn is_pbr_diffuse(st: Subtype) -> bool {
    matches!(st, Subtype::PbrAlbedo | Subtype::SkinAlbedo)
}

/// The slot card a resolved sub-type fills; `None` for sub-types without a card (env mask, complex `_m`,
/// `msn_specular`, backlight, `skin_wet`, …). Complex grass is one file in the Diffuse card.
pub fn slot_kind(st: Subtype) -> Option<SlotKind> {
    Some(match st {
        Subtype::PbrAlbedo
        | Subtype::Diffuse
        | Subtype::LandscapeDiffuse
        | Subtype::SkinAlbedo
        | Subtype::ComplexGrass => SlotKind::Diffuse,
        Subtype::PbrNormal | Subtype::Normal | Subtype::LandscapeNormal | Subtype::Msn => {
            SlotKind::Normal
        }
        Subtype::PbrEmissive | Subtype::Glow => SlotKind::Emissive,
        Subtype::SkinSk => SlotKind::SkinSk,
        Subtype::Height => SlotKind::Height,
        Subtype::Rmaos | Subtype::SkinRfaos => SlotKind::Rmaos,
        Subtype::CoatNormal => SlotKind::Coat,
        Subtype::PbrFuzz => SlotKind::Fuzz,
        Subtype::PbrSubsurface => SlotKind::Subsurface,
        _ => return None,
    })
}

/// The slot a texture fills, by its resolved sub-type. Raw packing sources and items without a job have none.
pub fn slot_of(item: &TextureItem) -> Option<SlotKind> {
    if matches!(item.status, ItemStatus::PackedInto(_)) || item.kind == ItemKind::Helper {
        return None;
    }
    slot_kind(item.job.as_ref()?.profile.subtype)
}

/// The slot cards shader `s` can use (F1 spec §5, DECISIONS V8): each role's sub-types mapped through `slot_kind`.
/// Roles matched by suffix only (`_e`, `_i`, `_sk` on Skin/Eye) map to none.
pub fn usable_slots(s: Shader) -> BTreeSet<SlotKind> {
    let spec = s.spec();
    spec.required
        .iter()
        .chain(spec.optional)
        .flat_map(|r| r.subtypes.iter().copied())
        .filter_map(slot_kind)
        .collect()
}

/// One slot card of the set editor (F1 spec §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotCell {
    pub kind: SlotKind,
    pub fill: SlotFill,
    /// False for a filled slot the set's shader does not use: it stays, outlined, and says why.
    pub usable: bool,
    /// Note AO: another filled kind in the same PGPatcher slot (`_f` beside `_cnr`, `_sk` beside `_g`): a conflict.
    pub clash: Option<SlotKind>,
}

/// The set's slot cards in PGPatcher slot order 1–8, one per slot (Note AO: slots 3 and 7 hold two kinds each; the
/// card holds the filled one, else the first in `SlotKind` order, and a second filled one is its `clash`). These
/// are every slot the picked shader can use, filled or empty, plus every filled slot it cannot use. With no pick yet,
/// only the filled slots show (F1 spec §5).
pub fn slot_row(pick: Option<&ShaderPick>, slots: &BTreeMap<SlotKind, SlotFill>) -> Vec<SlotCell> {
    let usable = pick.map(|p| usable_slots(p.shader));
    let filled = |k: &SlotKind| !matches!(slots.get(k), None | Some(SlotFill::Empty));
    let mut kinds: Vec<SlotKind> = SlotKind::ALL
        .into_iter()
        .filter(|k| filled(k) || usable.as_ref().is_some_and(|u| u.contains(k)))
        .collect();
    kinds.sort_by_key(|k| (k.slot_number(), *k));
    let mut cells: Vec<SlotCell> = vec![];
    for kind in kinds {
        let cell = SlotCell {
            kind,
            fill: slots.get(&kind).cloned().unwrap_or(SlotFill::Empty),
            usable: usable.as_ref().is_none_or(|u| u.contains(&kind)),
            clash: None,
        };
        match cells.last_mut() {
            Some(c) if c.kind.slot_number() == kind.slot_number() => {
                if filled(&c.kind) && filled(&kind) {
                    c.clash = Some(kind);
                } else if filled(&kind) {
                    *c = cell;
                }
            }
            _ => cells.push(cell),
        }
    }
    cells
}

/// Note AO: why a card's slot is in conflict (both kinds of slot 3 or 7 filled), if it is.
pub fn slot_clash_text(cell: &SlotCell) -> Option<String> {
    slot_conflict(&[cell.kind], cell.clash?)
}

/// Why `incoming` cannot join a set holding `existing`, if it cannot (spec §4.1: `_cnr` and `_f` share slot 7;
/// the emissive `_g` and the Advanced skin `_sk` share slot 3).
pub fn slot_conflict(existing: &[SlotKind], incoming: SlotKind) -> Option<String> {
    let clash = |other: SlotKind| existing.contains(&other);
    match incoming {
        SlotKind::Coat if clash(SlotKind::Fuzz) => Some(SLOT7_CLASH.into()),
        SlotKind::Fuzz if clash(SlotKind::Coat) => Some(SLOT7_CLASH.into()),
        SlotKind::Emissive if clash(SlotKind::SkinSk) => Some(SLOT3_CLASH.into()),
        SlotKind::SkinSk if clash(SlotKind::Emissive) => Some(SLOT3_CLASH.into()),
        _ => None,
    }
}

const SLOT7_CLASH: &str =
    "a set cannot hold both a coat normal (_cnr) and a fuzz map (_f): both use slot 7";
const SLOT3_CLASH: &str =
    "a set cannot hold both an emissive map (_g) and a skin _sk map: both use slot 3";

fn entry_of<'a>(entries: &'a [SetEntry], id: &str) -> Option<&'a SetEntry> {
    entries.iter().find(|e| same_id(&e.id, id))
}

fn dir_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The stem the diffuse's output has when it mirrors its source (DECISIONS N2): the source stem, without a colour
/// term (`rock01_albedo` → `rock01`, as Discover names the output). Taken from the source, never from the plan's
/// output, which an output path has already moved under the prefix.
fn mirrored_stem(item: &TextureItem) -> String {
    let stem = Path::new(&item.key)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    match split_vocab(&stem) {
        Some((base, v)) if v.role == VocabRole::Colour => base,
        _ => stem,
    }
}

/// Every set, in display order (audit 7, DECISIONS Note Y7): built once per change of its inputs.
pub fn views(m: &AppModel) -> Arc<Vec<SetView>> {
    let key = m.view_key();
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some(v) = &c.views {
                return v.clone();
            }
        }
    }
    let v = Arc::new(build_views(m));
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = ViewCache {
            key: Some(key),
            ..Default::default()
        };
    }
    c.views = Some(v.clone());
    v
}

#[cfg(test)]
thread_local! {
    /// Test probe: how many deep clones `set_views` made on this thread.
    pub static SET_VIEWS_CLONES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn set_views(m: &AppModel) -> Vec<SetView> {
    #[cfg(test)]
    SET_VIEWS_CLONES.with(|c| c.set(c.get() + 1));
    // shortcut: a clone per call for the ~20 callers that take ownership; switch them to `views` if it shows in a profile.
    (*views(m)).clone()
}

/// Every set, in display order, with its slots, ownership and prefix.
fn build_views(m: &AppModel) -> Vec<SetView> {
    let root = m.sets_root();
    let entries = m
        .sets_doc
        .as_ref()
        .map(|d| d.entries().to_vec())
        .unwrap_or_default();
    let order = m.sets_doc.as_ref().map(|d| d.order()).unwrap_or_default();

    let mut views: Vec<SetView> = m
        .sets
        .iter()
        .map(|s| {
            let mut v = SetView::new(s.key.clone(), set_id(&root, &s.key), s.members.clone());
            v.shader = s.shader.clone();
            v.set_warnings = s.warnings.clone();
            v.recipes = s.recipes.clone();
            for mk in &s.members {
                if let Some(slot) = m.item(mk).and_then(slot_of) {
                    v.slots
                        .entry(slot)
                        .or_insert(SlotFill::Own { item: mk.clone() });
                }
            }
            v
        })
        .collect();
    // Created sets that have no members yet, and entries whose files have all gone (spec §7.1: shown as missing,
    // never silently dropped).
    for e in &entries {
        if !views.iter().any(|v| same_id(&v.id, &e.id)) {
            views.push(SetView::new(String::new(), e.id.clone(), vec![]));
        }
    }
    // Order: the saved order first, then the rest by id.
    let pos = |v: &SetView| {
        order
            .iter()
            .position(|o| same_id(o, &v.id))
            .unwrap_or(usize::MAX)
    };
    views.sort_by(|a, b| pos(a).cmp(&pos(b)).then_with(|| a.id.cmp(&b.id)));
    // Entry data: created flag, settings, missing members, borrowed slots.
    let own_slots: Vec<(String, BTreeMap<SlotKind, SlotFill>)> = views
        .iter()
        .map(|v| (v.id.clone(), v.slots.clone()))
        .collect();
    for v in &mut views {
        let Some(e) = entry_of(&entries, &v.id) else {
            continue;
        };
        v.created = e.created;
        v.settings = e.settings.clone();
        v.shader_choice = e.shader;
        v.locks = e.lock.clone();
        if v.key.is_empty() && !e.created {
            v.missing.push(Missing::NoFilesLeft);
        }
        for f in e.add.iter().chain(&e.remove) {
            if !m.items.iter().any(|i| path_key_matches(f, &i.key)) {
                v.missing.push(Missing::File(f.clone()));
            }
        }
        for (slot, from) in &e.borrow {
            let borrow = |empty| Missing::Borrow {
                slot: *slot,
                from: from.clone(),
                empty,
            };
            let Some((_, src)) = own_slots.iter().find(|(id, _)| same_id(id, from)) else {
                v.missing.push(borrow(false));
                continue;
            };
            let hit = src
                .iter()
                .find(|(k, f)| k.slot_number() == *slot && matches!(f, SlotFill::Own { .. }));
            if let Some((kind, SlotFill::Own { item })) = hit {
                v.slots.insert(
                    *kind,
                    SlotFill::Borrowed {
                        from: from.clone(),
                        item: item.clone(),
                    },
                );
            } else {
                v.missing.push(borrow(true));
            }
        }
    }
    // PBR, derived match, prefix.
    for v in &mut views {
        let diffuse = match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item: k } | SlotFill::Borrowed { item: k, .. }) => m.item(k),
            _ => None,
        };
        let st = diffuse
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype);
        // AG90: everything that reads `pbr` is PGPatcher JSON (entries, blocks, pitfalls, prefixes, ownership, rename).
        v.pbr = st == Some(Subtype::PbrAlbedo);
        v.skin = st == Some(Subtype::SkinAlbedo);
        if is_pbr_grass(v) {
            crate::pgp::settings::grass_view(&mut v.settings);
        }
        v.derived_match = diffuse.map(|i| derive_match_diffuse(dir_of(&i.key), &mirrored_stem(i)));
        match prefix_of(&v.settings, v.derived_match.as_deref().unwrap_or("")) {
            Ok(p) => v.prefix = Some(p),
            Err(e) => v.prefix_error = Some(e.0),
        }
    }
    // Ownership marks.
    let owners = owners(&views);
    for v in &mut views {
        v.owns = owners
            .iter()
            .filter(|((_, users), owner)| {
                owner.as_deref() == Some(v.id.as_str()) && users.len() > 1
            })
            .filter_map(|((item, _), _)| {
                v.slots
                    .iter()
                    .find(|(_, f)| matches!(f, SlotFill::Own { item: i } | SlotFill::Borrowed { item: i, .. } if i == item))
                    .map(|(k, _)| *k)
            })
            .collect();
    }
    views
}

/// For each used output item: (item, the ids of the sets using it in display order) → the owner id, the first PBR
/// set among them (spec §3.3); `None` when no user is a PBR set.
type Ownership = Vec<((String, Vec<String>), Option<String>)>;

fn owners(views: &[SetView]) -> Ownership {
    let mut users: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    for v in views {
        for f in v.slots.values() {
            if let SlotFill::Own { item } | SlotFill::Borrowed { item, .. } = f {
                users
                    .entry(item.clone())
                    .or_default()
                    .push((v.id.clone(), v.pbr));
            }
        }
    }
    users
        .into_iter()
        .map(|(item, us)| {
            let owner = us.iter().find(|(_, pbr)| *pbr).map(|(id, _)| id.clone());
            ((item, us.into_iter().map(|(id, _)| id).collect()), owner)
        })
        .collect()
}

/// The suffix of `item`'s output under a set prefix (M3b spec: `<prefix><canonical suffix>.dds`): its slot's,
/// except that an `_rfaos` keeps `rfaos` (the packed file suffix stays `_rfaos`, DECISIONS Note R-skin-1), where
/// the RMAOS slot's suffix would rename it `_rmaos` (final review C1).
fn output_suffix(item: &TextureItem, slot: SlotKind) -> &'static str {
    match item.job.as_ref().map(|j| j.profile.subtype) {
        Some(Subtype::SkinRfaos) => "rfaos",
        _ => slot.suffix(),
    }
}

/// The recipe suffix of a packed item (`rmaos`, `rfaos`, `p`, `m`).
fn recipe_suffix(i: &TextureItem) -> Option<String> {
    let pk = i.job.as_ref()?.pack.as_ref()?;
    Some(pk.kind.suffix().to_string())
}

/// The engine inputs of the current documents and plan (spec §2.1, DECISIONS P1–P2).
pub fn engine_inputs(m: &AppModel) -> (Vec<SetEdit>, BTreeMap<OutputKey, PathBuf>) {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    let mut edits = vec![];
    for e in entries.iter().filter(|e| e.created) {
        edits.push(SetEdit::Create { set: e.id.clone() });
    }
    for e in entries.iter() {
        if let Some(from) = &e.renamed_from {
            edits.push(SetEdit::Rename {
                from: from.clone(),
                to: e.id.clone(),
            });
        }
    }
    for e in entries.iter() {
        for f in &e.add {
            edits.push(SetEdit::Move {
                file: f.clone(),
                to: e.id.clone(),
            });
        }
    }
    for e in entries.iter() {
        for f in &e.remove {
            edits.push(SetEdit::Remove { file: f.clone() });
        }
    }
    let mut paths = BTreeMap::new();
    let views = set_views(m);
    // F3b D2: a set's `output_name` on any target; on CS a PBR set's prefix names its outputs instead.
    let named = output_name_paths(m, &views);
    if m.target != Target::Cs {
        paths.extend(named);
        return (edits, paths);
    }
    let candidates = output_candidates(m, &views);
    // Mirrored outputs: textures with no explicit path, now or in the plan on screen, write their job key. An owned
    // path on one is held back too (the engine would skip both as IN-DUPLICATE and the plan would not settle);
    // `path_collisions` reports it.
    let explicit = |k: &OutputKey| {
        candidates.iter().any(|(_, c, _)| c == k)
            || named.iter().any(|(c, _)| c == k)
            || m.last_inputs
                .as_ref()
                .is_some_and(|l| l.output_paths.contains_key(k))
    };
    let mirrored: BTreeSet<String> = m
        .items
        .iter()
        // Only a texture that will be written (skipped and failed files have no job, as in `path_collisions`).
        .filter(|it| it.job.is_some() && !it.job_key.is_empty())
        .filter(|it| {
            let k = item_output_key(&views, it).unwrap_or(OutputKey::File(it.key.clone()));
            !explicit(&k)
        })
        .map(|it| it.job_key.replace('\\', "/").to_lowercase())
        .collect();
    let mut seen = BTreeSet::new();
    let named = named.into_iter().map(|(k, rel)| (String::new(), k, rel));
    for (_, key, rel) in named.chain(candidates) {
        if mirrored.contains(&rel.to_string_lossy().replace('\\', "/").to_lowercase()) {
            continue;
        }
        // Two outputs on one path: keep the first in display order. Export is blocked by `path_collisions` (Task 11),
        // and the engine never sees the collision, so the plan is stable. (Sending both would make the engine skip
        // both as IN-DUPLICATE, the sets would stop being PBR sets, the paths would vanish, and the two-pass planning
        // would flip between two plans.)
        if seen.insert(rel.to_string_lossy().replace('\\', "/").to_lowercase()) {
            paths.insert(key, rel);
        }
    }
    (edits, paths)
}

/// F3b D2: the explicit path of every output of each set with an `output_name`: the output's own folder and Skyrim
/// suffix, the new base name. A PBR set on CS is left out (its prefix and PGPatcher `rename` name its outputs), and
/// so is a name `check_output_name` refuses (a hand edit).
pub fn output_name_paths(m: &AppModel, views: &[SetView]) -> Vec<(OutputKey, PathBuf)> {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    let mut out = vec![];
    for e in entries.iter() {
        let Some(name) = e.output_name.as_deref() else {
            continue;
        };
        if check_output_name(name).is_err() {
            continue;
        }
        let Some(v) = views.iter().find(|v| same_id(&v.id, &e.id)) else {
            continue;
        };
        if v.pbr && m.target == Target::Cs {
            continue;
        }
        for key in &v.members {
            let Some(item) = m.item(key) else { continue };
            if item.job.is_none() || item.job_key.is_empty() {
                continue;
            }
            let Some(rel) = renamed_output(&item.job_key, name) else {
                continue;
            };
            let k = if item.kind == ItemKind::Packed {
                let Some(suffix) = recipe_suffix(item) else {
                    continue;
                };
                OutputKey::Packed {
                    set: v.id.clone(),
                    suffix,
                }
            } else {
                OutputKey::File(item.key.clone())
            };
            out.push((k, rel));
        }
    }
    out
}

/// `job_key` (an output path relative to the export root) with its base name replaced by `name`: the folder and the
/// suffix stay (`armor/wall_n.dds`, `steel` → `armor/steel_n.dds`). Applying it twice gives the same path,
/// because `check_output_name` refuses a name that ends in a suffix.
pub fn renamed_output(job_key: &str, name: &str) -> Option<PathBuf> {
    let p = Path::new(job_key);
    let stem = p.file_stem()?.to_string_lossy();
    // Everything after the base stays: a Skyrim suffix, or an exporter term an output keeps (`_roughness` on a
    // target that packs nothing), so two members never land on one name.
    let base = base_of(&stem);
    let file = format!("{name}{}.dds", &stem[base.len().min(stem.len())..]);
    Some(match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.join(file),
        _ => PathBuf::from(file),
    })
}

/// F3b: whether `name` can be an output base name. A name ending in a map suffix (`wall_n`) is refused: the
/// suffix belongs to each output, and the name would be read back as a base plus that suffix.
pub fn check_output_name(name: &str) -> Result<(), String> {
    check_named(name, "output name")
}

/// `check_output_name` for a named field (the PGPatcher `rename` box shares it).
pub fn check_named(name: &str, field: &str) -> Result<(), String> {
    check_file_name(name, field).map_err(|e| e.0)?;
    let base = base_of(name);
    if base.len() < name.len() {
        return Err(format!(
            "{field}: ends in {}, a map suffix; each output keeps its own ({name:?})",
            &name[base.len()..]
        ));
    }
    Ok(())
}

/// Ids of every set whose Rmaos slot (`_rmaos`/`_rfaos`) holds an output another set's Rmaos slot holds too, owner
/// and borrowers alike (M3b spec §3.3, v1 instruction; DECISIONS R7). The engine bakes no normal variance into a
/// shared roughness and reports no missing companion for these sets. Empty off `cs`.
pub fn shared_roughness(m: &AppModel) -> BTreeSet<String> {
    if m.target != Target::Cs {
        return BTreeSet::new();
    }
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in set_views(m) {
        if let Some(SlotFill::Own { item } | SlotFill::Borrowed { item, .. }) =
            v.slots.get(&SlotKind::Rmaos)
        {
            users.entry(item.clone()).or_default().push(v.id.clone());
        }
    }
    users
        .into_values()
        .filter(|ids| ids.len() > 1)
        .flatten()
        .collect()
}

/// Borrower id → owner id for every set whose Normal slot borrows another set's `_n` (DECISIONS Note R
/// M4-final-1). The engine bakes the borrower's own roughness member from that `_n`. Empty off `cs`.
pub fn borrowed_normal(m: &AppModel) -> BTreeMap<String, String> {
    if m.target != Target::Cs {
        return BTreeMap::new();
    }
    let views = set_views(m);
    views
        .iter()
        .filter_map(|v| match v.slots.get(&SlotKind::Normal) {
            Some(SlotFill::Borrowed { from, .. }) => {
                // The owner's id as the engine knows it (`from` is matched case-insensitively in `set_views`).
                let owner = views.iter().find(|o| same_id(&o.id, from))?;
                Some((v.id.clone(), owner.id.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Entry id → the saved shader choice (M6, `BatchRequest::set_shaders`).
pub fn set_shaders(m: &AppModel) -> BTreeMap<String, Shader> {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    entries
        .iter()
        .filter_map(|e| Some((e.id.clone(), e.shader?)))
        .collect()
}

/// Note BM: the ids of the sets with a `[sets.cs_txst]` block (sent as `BatchRequest::cs_txst_sets`).
pub fn cs_txst_sets(m: &AppModel) -> BTreeSet<String> {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    entries
        .iter()
        .filter(|e| e.cs_txst.is_some())
        .map(|e| e.id.clone())
        .collect()
}

/// The Shader dropdown's choices for `target` (after "Auto").
pub fn shader_options(target: Target) -> Vec<Shader> {
    Shader::available(target).to_vec()
}

/// Every owned output's explicit path, sorted by its owner's display position: (owner id, key, path relative to
/// the export root). Duplicate paths are kept here; `engine_inputs` drops them and `path_collisions` reports them.
/// A path that fails `check_rel_path` is never a candidate: the engine joins these under the export folder as given.
pub fn output_candidates(m: &AppModel, views: &[SetView]) -> Vec<(String, OutputKey, PathBuf)> {
    let pos = |id: &str| views.iter().position(|v| v.id == id).unwrap_or(usize::MAX);
    let mut out = vec![];
    for ((item_key, users), owner) in owners(views) {
        let Some(owner) = owner else { continue };
        let Some(ov) = views.iter().find(|v| v.id == owner) else {
            continue;
        };
        let Some(prefix) = &ov.prefix else { continue };
        let Some(item) = m.item(&item_key) else {
            continue;
        };
        let Some(slot) = slot_of(item) else { continue };
        let rel = output_rel(prefix, output_suffix(item, slot));
        if check_rel_path(&rel.to_string_lossy(), "output path").is_err() {
            continue;
        }
        let key = if item.kind == ItemKind::Packed {
            // The set holding the item (its source set) names a packed output.
            let source = users
                .iter()
                .find(|id| {
                    views.iter().any(|v| {
                        &v.id == *id
                            && matches!(v.slots.get(&slot), Some(SlotFill::Own { item }) if *item == item_key)
                    })
                })
                .cloned()
                .unwrap_or(owner.clone());
            let Some(suffix) = recipe_suffix(item) else {
                continue;
            };
            OutputKey::Packed {
                set: source,
                suffix,
            }
        } else {
            OutputKey::File(item.key.clone())
        };
        out.push((owner, key, rel));
    }
    out.sort_by_key(|(o, _, _)| pos(o));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropChoice {
    Move,
    Share,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropOutcome {
    Joined,
    AskReplace { existing: String },
    AskChoice { from: String },
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotCommand {
    FromSet,
    Path(String),
    Clear,
}

const READ_ONLY: &str = "sdc-project.toml is read-only";
const PACKED_MOVE: &str =
    "a packed map follows its raw maps; move those, or share the finished map";
/// The `missing` line of an `[[sets]]` entry whose files have all gone.
const NO_FILES_LEFT: &str = "no texture of this set is left in the folder";

/// Match-related keys change output paths, so editing them re-plans (DECISIONS P7).
fn replans(key: &str) -> bool {
    key.starts_with("match") || key == "path_contains" || key == "rename"
}

/// F3a Task 8: why a locked slot refuses a slot command or a bulk edit.
pub const SLOT_LOCKED: &str = "slot locked";

/// Why `slot` of `v` takes no change: it is locked (F3a Task 8).
fn lock_refusal(v: &SetView, slot: Option<SlotKind>) -> Result<(), String> {
    match slot.map(SlotKind::slot_number) {
        Some(n) if v.locks.contains(&n) => {
            Err(format!("slot {n} of {} is locked; unlock it first", v.id))
        }
        _ => Ok(()),
    }
}

/// Note DW3: the items in a locked slot of their own set (`BatchRequest::pinned_files`); a borrowing set's lock is
/// left out, so it never changes the owner's reading.
pub fn pinned_files(m: &AppModel) -> BTreeSet<String> {
    locked_items(m)
        .into_iter()
        .filter(|(_, why)| why == SLOT_LOCKED)
        .map(|(k, _)| k)
        .collect()
}

/// The items filling a locked slot of any set (F3a Task 8), with the reason bulk edits skip them: "slot locked" in
/// the set that owns the texture, else "slot locked in <set>" for a set borrowing it. The own lock wins.
pub fn locked_items(m: &AppModel) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for v in views(m).iter() {
        for (k, f) in &v.slots {
            if !v.locks.contains(&k.slot_number()) {
                continue;
            }
            match f {
                SlotFill::Own { item } => {
                    out.insert(item.clone(), SLOT_LOCKED.to_string());
                }
                SlotFill::Borrowed { item, .. } => {
                    out.entry(item.clone())
                        .or_insert_with(|| format!("{SLOT_LOCKED} in {}", v.id));
                }
                SlotFill::Empty => {}
            }
        }
    }
    out
}

/// What `slot_gives` says for a slot the set has no file for.
pub const NOTHING: &str = "nothing";

/// What set `set` itself gives at slot `n` (the Slot commands window, the slot menu): the file names of its own
/// slot cards there, or "nothing".
pub fn slot_gives(m: &AppModel, set: &str, n: u8) -> String {
    let views = views(m);
    let Some(v) = views.iter().find(|v| same_id(&v.id, set)) else {
        return String::new();
    };
    let names: Vec<String> = SlotKind::ALL
        .iter()
        .filter(|s| s.slot_number() == n)
        .filter_map(|s| match v.slots.get(s)? {
            SlotFill::Own { item } => m.item(item).map(|i| i.name.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        NOTHING.into()
    } else {
        names.join(", ")
    }
}

/// Why a member outside a `textures` folder cannot be linked (F3a Task 8).
pub const NO_GAME_PATH: &str = "no game path (not under a textures folder)";

/// F3a Task 8: the path the game loads `item`'s output from, in the form Path… stores (relative to Data\,
/// backslashes, `.dds`), from the current documents: its planned output path when it has one (a PBR output under
/// `textures\pbr\`, as the automatic slot paths, DECISIONS P11), else its game path (`textures\…`). A member with no
/// `textures` folder above it has none.
pub fn member_game_path(m: &AppModel, item: &str) -> Result<String, String> {
    let paths = engine_inputs(m).1;
    game_path_in(m, &views(m), &paths, item)
}

/// `member_game_path` over views and output paths already built (the slot menu builds them once).
pub(crate) fn game_path_in(
    m: &AppModel,
    views: &[SetView],
    paths: &BTreeMap<OutputKey, PathBuf>,
    item: &str,
) -> Result<String, String> {
    let it = m.item(item).ok_or_else(|| format!("no texture {item}"))?;
    let job = it
        .job
        .as_ref()
        .ok_or_else(|| format!("{} writes no output", it.name))?;
    if let Some(p) = item_output_key(views, it).and_then(|k| paths.get(&k)) {
        return Ok(p.to_string_lossy().replace('/', "\\"));
    }
    // `game_path` starts at the last `textures` folder, else it is the path under the opened folder.
    if !job.game_path.starts_with("textures/") {
        return Err(NO_GAME_PATH.into());
    }
    Ok(written_game_path(job).replace('/', "\\"))
}

/// The game path a member's export writes (wave 2 #4, Note R9): the folder of `game_path` and the file name of
/// `output`, as the engine's `colour_mesh_key` and `gras_key` take them (`leaf_normal.png` writes `leaf_n.dds`).
/// `/`-separated.
pub(crate) fn written_game_path(job: &engine::policy::ResolvedJob) -> String {
    let name = job.output.file_name().unwrap_or_default().to_string_lossy();
    match job.game_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.into_owned(),
    }
}

/// Empty `slot` of set `id` in the document: its own occupant leaves the set, a borrow is dropped. Part of a replace.
fn clear_slot(s: &mut SetsDoc, id: &str, slot: Option<SlotKind>, existing: &Option<SlotFill>) {
    match existing {
        Some(SlotFill::Own { item }) => s.remove_member(id, item),
        Some(SlotFill::Borrowed { .. }) => {
            if let Some(n) = slot.map(SlotKind::slot_number) {
                s.set_borrow(id, n, None);
            }
        }
        _ => {}
    }
}

impl AppModel {
    pub(super) fn view_of(&self, id: &str) -> Option<SetView> {
        views(self).iter().find(|v| same_id(&v.id, id)).cloned()
    }

    fn set_of_item(&self, item: &str) -> Option<SetView> {
        set_views(self)
            .into_iter()
            .find(|v| v.members.iter().any(|k| k == item))
    }

    pub(crate) fn sets_editable(&self) -> bool {
        self.sets_doc.as_ref().is_some_and(|d| !d.read_only())
    }

    /// Whether slot `n` of set `set` is locked (F3a Task 8).
    pub fn slot_locked(&self, set: &str, n: u8) -> bool {
        views(self)
            .iter()
            .any(|v| same_id(&v.id, set) && v.locks.contains(&n))
    }

    /// Lock (or unlock) slot `n` of set `set`, as one step (F3a Task 8), re-planned: the engine reads the pinned
    /// files (Note DW).
    pub fn set_slot_lock(&mut self, set: &str, n: u8, on: bool) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        if !(1..=8).contains(&n) {
            return Err(format!("slot{n}: there are slots 1 to 8"));
        }
        let id = self.view_of(set).ok_or_else(|| format!("no set {set}"))?.id;
        let label = if on { "Lock" } else { "Unlock" };
        self.edit_docs(
            &format!("{label} slot {n} of {id}"),
            None,
            true,
            move |_, d| d.set_lock(&id, n, on),
        );
        Ok(())
    }

    /// Note AN: turn slot `n`'s PGPatcher lock key of set `set` on (`true`) or off (the key removed), as one step.
    pub fn set_pgp_lock(&mut self, set: &str, n: u8, on: bool) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        let key = crate::pgp::prefix::lock_key(n)
            .ok_or_else(|| format!("slot{n} has no PGPatcher lock that keeps its texture"))?;
        let id = self.view_of(set).ok_or_else(|| format!("no set {set}"))?.id;
        self.set_json_value(&id, key, on.then_some(Value::Bool(true)));
        Ok(())
    }

    /// Why `item` cannot leave its set: it fills a locked slot there (F3a Task 8).
    fn leaves_locked(&self, item: &str) -> Result<(), String> {
        let Some(v) = self.set_of_item(item) else {
            return Ok(());
        };
        let slot = v
            .slots
            .iter()
            .find(|(_, f)| matches!(f, SlotFill::Own { item: i } if i == item))
            .map(|(k, _)| *k);
        lock_refusal(&v, slot)
    }

    fn is_packed(&self, item: &str) -> bool {
        self.item(item).is_some_and(|i| i.kind == ItemKind::Packed)
    }

    /// The set ids the engine knows or will know (lower-case): the sets on screen, and the original names of renamed
    /// sets (the rename applies first in the engine, but another set taking that name would make it fail).
    fn taken_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = set_views(self)
            .into_iter()
            .map(|v| fold_id(&v.id))
            .collect();
        if let Some(d) = &self.sets_doc {
            ids.extend(
                d.entries()
                    .iter()
                    .filter_map(|e| e.renamed_from.as_deref().map(fold_id)),
            );
        }
        ids
    }

    /// The set a texture may be dropped on: it exists and still has files (or is a set the user created).
    fn drop_target(&self, set: &str) -> Result<SetView, String> {
        let target = self.view_of(set).ok_or_else(|| format!("no set {set}"))?;
        if target.key.is_empty() && !target.created {
            return Err(format!("{}: {NO_FILES_LEFT}", target.id));
        }
        Ok(target)
    }

    /// What fills `slot` of `target` now: `None` when the drop needs no replace. A packed occupant cannot leave (it
    /// follows its raw maps); a borrow of the same set's output is already there (`None`).
    fn occupant(
        &self,
        target: &SetView,
        slot: Option<SlotKind>,
        share_from: Option<&str>,
    ) -> Result<Option<SlotFill>, String> {
        lock_refusal(target, slot)?;
        match slot.and_then(|s| target.slots.get(&s)).cloned() {
            Some(SlotFill::Own { item }) if self.is_packed(&item) => Err(format!(
                "that slot holds a packed map ({item}); move its raw maps out of the set first"
            )),
            Some(SlotFill::Borrowed { from, .. })
                if share_from.is_some_and(|f| same_id(f, &from)) =>
            {
                Ok(None)
            }
            Some(f @ (SlotFill::Own { .. } | SlotFill::Borrowed { .. })) => Ok(Some(f)),
            _ => Ok(None),
        }
    }

    /// A texture dropped on set `set` (spec §3.1, §3.3).
    pub fn drop_on_set(&mut self, item: &str, set: &str) -> DropOutcome {
        if !self.sets_editable() {
            return DropOutcome::Refused(READ_ONLY.into());
        }
        let target = match self.drop_target(set) {
            Ok(t) => t,
            Err(e) => return DropOutcome::Refused(e),
        };
        let from = self.set_of_item(item);
        if from.as_ref().is_some_and(|f| f.id == target.id) {
            return DropOutcome::Joined;
        }
        let incoming = self.item(item).and_then(slot_of);
        let existing: Vec<SlotKind> = target.slots.keys().copied().collect();
        if let Some(why) = incoming.and_then(|s| slot_conflict(&existing, s)) {
            return DropOutcome::Refused(why);
        }
        if let Err(why) = lock_refusal(&target, incoming) {
            return DropOutcome::Refused(why);
        }
        // A texture that belongs to a real set (more than itself, or one the user touched) asks what to do. An entry
        // holding only its id (left by a rename back) is no edit.
        if let Some(f) = &from {
            let edited = self.sets_doc.as_ref().is_some_and(|d| {
                entry_of(&d.entries(), &f.id).is_some_and(|e| {
                    *e != SetEntry {
                        id: e.id.clone(),
                        ..SetEntry::default()
                    }
                })
            });
            if f.members.len() > 1 || edited || f.created {
                return DropOutcome::AskChoice { from: f.id.clone() };
            }
        }
        self.drop_choice(item, set, DropChoice::Move)
    }

    /// The answer to `AskChoice` (or a plain move). Any choice that would put a finished texture into a slot the set
    /// already fills asks to replace first (spec §3.1); answer that with `drop_replace` and the same choice.
    pub fn drop_choice(&mut self, item: &str, set: &str, choice: DropChoice) -> DropOutcome {
        if !self.sets_editable() {
            return DropOutcome::Refused(READ_ONLY.into());
        }
        let target = match self.drop_target(set) {
            Ok(t) => t,
            Err(e) => return DropOutcome::Refused(e),
        };
        let slot = self.item(item).and_then(slot_of);
        let from = self.set_of_item(item).map(|f| f.id);
        let share_from = (choice == DropChoice::Share)
            .then_some(from.as_deref())
            .flatten();
        match self.occupant(&target, slot, share_from) {
            Err(e) => return DropOutcome::Refused(e),
            Ok(Some(
                SlotFill::Own { item: existing } | SlotFill::Borrowed { item: existing, .. },
            )) => {
                // Checked first so a refusal (a packed map, a raw map to share) is not asked about.
                if let Err(e) = self.choice_allowed(item, slot, choice) {
                    return DropOutcome::Refused(e);
                }
                return DropOutcome::AskReplace { existing };
            }
            Ok(_) => {}
        }
        match self.apply_choice(item, &target, slot, choice, None) {
            Ok(()) => DropOutcome::Joined,
            Err(e) => DropOutcome::Refused(e),
        }
    }

    /// The answer to `AskReplace` for `choice`: the occupant leaves (it becomes a set of its own, or the borrow is
    /// dropped) and `item` comes in by `choice`, as one step. The reason comes back when it is refused.
    pub fn drop_replace(
        &mut self,
        item: &str,
        set: &str,
        choice: DropChoice,
    ) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        let target = self.drop_target(set)?;
        let slot = self.item(item).and_then(slot_of);
        let from = self.set_of_item(item).map(|f| f.id);
        let share_from = (choice == DropChoice::Share)
            .then_some(from.as_deref())
            .flatten();
        let existing = self.occupant(&target, slot, share_from)?;
        self.apply_choice(item, &target, slot, choice, existing)
    }

    /// Why `choice` cannot bring `item` in, if it cannot (whatever the slot holds).
    fn choice_allowed(
        &self,
        item: &str,
        slot: Option<SlotKind>,
        choice: DropChoice,
    ) -> Result<(), String> {
        if choice == DropChoice::Move {
            self.leaves_locked(item)?;
        }
        match choice {
            DropChoice::Move if self.is_packed(item) => Err(PACKED_MOVE.into()),
            DropChoice::Share if slot.is_none() => Err(
                "only a finished texture can be shared; duplicate the source to use it in two sets"
                    .into(),
            ),
            DropChoice::Share if self.set_of_item(item).is_none() => {
                Err("the texture is in no set".into())
            }
            DropChoice::Duplicate if self.is_packed(item) => {
                Err("a packed map cannot be duplicated; duplicate its raw maps".into())
            }
            _ => Ok(()),
        }
    }

    /// Bring `item` into `target` by `choice`, first emptying the slot of `existing` (a replace). One step.
    fn apply_choice(
        &mut self,
        item: &str,
        target: &SetView,
        slot: Option<SlotKind>,
        choice: DropChoice,
        existing: Option<SlotFill>,
    ) -> Result<(), String> {
        self.choice_allowed(item, slot, choice)?;
        let id = target.id.clone();
        match choice {
            DropChoice::Move => {
                let it = item.to_string();
                let label = if existing.is_some() {
                    "Replace in"
                } else {
                    "Move into"
                };
                self.edit_docs(&format!("{label} {id}"), None, true, move |_, s| {
                    clear_slot(s, &id, slot, &existing);
                    s.add_member(&id, &it);
                });
                Ok(())
            }
            DropChoice::Share => {
                // Sharing adds no member: a set whose only texture leaves would vanish from the engine's sets.
                if matches!(&existing, Some(SlotFill::Own { .. }))
                    && target.members.len() == 1
                    && !target.created
                {
                    return Err(format!(
                        "{id} would have no texture of its own left; move another texture in first"
                    ));
                }
                let fid = self.set_of_item(item).map(|f| f.id).unwrap_or_default();
                let n = slot.map(SlotKind::slot_number).unwrap_or_default();
                self.edit_docs(&format!("Share into {id}"), None, true, move |_, s| {
                    clear_slot(s, &id, slot, &existing);
                    s.set_borrow(&id, n, Some(&fid));
                });
                Ok(())
            }
            DropChoice::Duplicate => self.duplicate_into(item, &id, slot, existing),
        }
    }

    /// A texture dropped on the empty area: a new set named after its base name (spec §3.1). Returns the new id;
    /// the reason when nothing was done (read-only `sdc-project.toml`, a packed map, which follows its raw maps, or a
    /// texture filling a locked slot).
    pub fn drop_new_set(&mut self, item: &str) -> Result<String, String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        if self.is_packed(item) {
            return Err(PACKED_MOVE.into());
        }
        self.leaves_locked(item)?;
        let stem = Path::new(item)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let base = base_of(&stem);
        let dir = dir_of(item);
        let first = if dir.is_empty() {
            base
        } else {
            format!("{dir}/{base}")
        };
        let taken = self.taken_ids();
        let mut id = first.clone();
        let mut n = 2;
        while taken.contains(&fold_id(&id)) {
            id = format!("{first}-{n}");
            n += 1;
        }
        let (i, it) = (id.clone(), item.to_string());
        self.edit_docs(&format!("New set {id}"), None, true, move |_, s| {
            s.create_set(&i);
            s.add_member(&i, &it);
        });
        self.note_new_set(&id);
        Ok(id)
    }

    /// Copy `item`'s source next to it under set `set`'s base name and move the copy in (spec §3.4); `existing`
    /// first leaves `slot` (a replace).
    fn duplicate_into(
        &mut self,
        item: &str,
        set: &str,
        slot: Option<SlotKind>,
        existing: Option<SlotFill>,
    ) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        if self.is_packed(item) {
            return Err("a packed map cannot be duplicated; duplicate its raw maps".into());
        }
        let src = self
            .item(item)
            .map(|i| i.input.clone())
            .ok_or("no such texture")?;
        let root = self.root.clone().ok_or("no folder open")?;
        let stem = src
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        // `base_of` returns a prefix of `stem`, so the cut is on a char boundary.
        let tail = stem[base_of(&stem).len()..].to_string(); // e.g. "_roughness"
        let set_base = set.rsplit('/').next().unwrap_or(set).to_string();
        let dir = src
            .parent()
            .ok_or("the source has no folder")?
            .to_path_buf();
        let name = |b: &str| dir.join(format!("{b}{tail}.{ext}"));
        // #39: a name another file is read as (its alias) is taken too, as in the rename dialog.
        let aliases: BTreeSet<String> = self
            .project
            .as_ref()
            .map(|p| {
                self.items
                    .iter()
                    .filter_map(|i| p.read_as(&i.key))
                    .map(|a| a.replace('\\', "/").to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let aliased = |p: &Path| {
            p.strip_prefix(&root).is_ok_and(|r| {
                aliases.contains(&r.to_string_lossy().replace('\\', "/").to_lowercase())
            })
        };
        let mut dst = name(&set_base);
        let mut n = 2;
        while dst.exists() || aliased(&dst) {
            dst = name(&format!("{set_base}-{n}"));
            n += 1;
        }
        let key = dst
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .map_err(|_| "the copy would be outside the folder".to_string())?;
        std::fs::copy(&src, &dst).map_err(|e| format!("could not copy {}: {e}", src.display()))?;
        let Some(sha256) = sha256_of(&dst) else {
            let _ = std::fs::remove_file(&dst);
            return Err("could not read the copy".into());
        };
        let id = set.to_string();
        let before = self.sets_doc.as_ref().map(|d| d.text());
        self.edit_docs(&format!("Duplicate into {id}"), None, true, move |_, s| {
            clear_slot(s, &id, slot, &existing);
            s.add_member(&id, &key);
        });
        // A changed document means `edit_docs` pushed a new step (no merge key): that step, and only it, carries
        // the file.
        let recorded = self.sets_doc.as_ref().map(|d| d.text()) != before;
        match self.history.undo.last_mut().filter(|_| recorded) {
            Some(last) => {
                last.created_file = Some(CreatedFile {
                    path: dst,
                    source: src,
                    sha256,
                });
                Ok(())
            }
            None => {
                let _ = std::fs::remove_file(&dst);
                Err("the set did not change".into())
            }
        }
    }

    /// Move set `id` to display position `to` (the order decides ownership, so output paths: plan again, P7).
    /// Move set `id` to position `to`; false when nothing moved (trace-debug #55: no step, no dirty document).
    pub fn reorder_set(&mut self, id: &str, to: usize) -> bool {
        if !self.sets_editable() {
            return false;
        }
        let mut ids: Vec<String> = set_views(self).into_iter().map(|v| v.id).collect();
        let Some(from) = ids.iter().position(|i| same_id(i, id)) else {
            return false;
        };
        let moved = ids.remove(from);
        let to = to.min(ids.len());
        if to == from {
            return false;
        }
        ids.insert(to, moved);
        self.edit_docs("Reorder sets", None, true, move |_, s| s.set_order(&ids));
        true
    }

    /// Rename set `id` to `new_id`; refused (returns the reason) when the name is unusable or taken.
    pub fn rename_set(&mut self, id: &str, new_id: &str) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        let new_id = new_id.trim().replace('\\', "/");
        let new_id = new_id.trim_matches('/').to_string();
        check_rel_path(&new_id, "set name").map_err(|e| e.0)?;
        let Some(v) = self.view_of(id) else {
            return Err(format!("no set {id}"));
        };
        if same_id(&v.id, &new_id) {
            return Err(format!("the set is already named {}", v.id));
        }
        // The set's own original name is free: renaming back undoes the rename.
        let own_original = self.sets_doc.as_ref().is_some_and(|d| {
            entry_of(&d.entries(), &v.id)
                .and_then(|e| e.renamed_from.as_deref())
                .is_some_and(|f| same_id(f, &new_id))
        });
        if !own_original && self.taken_ids().contains(&fold_id(&new_id)) {
            return Err(format!("a set {new_id} already exists"));
        }
        let (a, b) = (v.id.clone(), new_id);
        // TA31: the Also export opt-out (`[export] skip`, set ids) follows, in the same step.
        self.edit_docs(&format!("Rename {a}"), None, true, move |p, s| {
            s.rename_set(&a, &b);
            p.rename_skip_also(&a, &b);
        });
        Ok(())
    }

    /// Delete a set the user created, or the entry of a set whose files have all gone (DECISIONS P2–P3: neither
    /// comes back from the file names); false (nothing done) for a live automatic set.
    pub fn delete_set(&mut self, id: &str) -> bool {
        if !self.sets_editable()
            || !self
                .view_of(id)
                .is_some_and(|v| v.created || v.key.is_empty())
        {
            return false;
        }
        let i = id.to_string();
        self.edit_docs(&format!("Delete {id}"), None, true, move |_, s| {
            s.delete_set(&i)
        });
        true
    }

    /// Note FC5 (supersedes AG13's confirm): the set menu's Delete set: no question, one undo step and a toast
    /// `Deleted set {id} ({n} members)` with Undo.
    pub fn delete_set_with_toast(&mut self, id: &str, members: usize) {
        if !self.delete_set(id) {
            return;
        }
        let step = (self.history.undo.len(), self.history.pushes);
        let s = if members == 1 { "" } else { "s" };
        self.messages.notify_with(
            super::messages::Severity::Info,
            format!("Deleted set {id} ({members} member{s})"),
            Some(super::messages::ToastAction::UndoDeleteSet(step.0, step.1)),
        );
    }

    /// The Delete set toast's Undo: undoes the step only while it is still the top one (nothing done since).
    pub fn undo_delete_set(&mut self, depth: usize, pushes: u64) {
        if self.history.undo.len() == depth && self.history.pushes == pushes {
            self.undo();
        } else {
            self.messages.notify(
                super::messages::Severity::Info,
                "Not undone: other changes came after the delete; use Undo in the header",
            );
        }
    }

    /// Take `item` out of set `id` (it becomes a set of its own, or returns to the automatic grouping when it was
    /// moved in). Refused for a packed map (it follows its raw maps) and for the only member of a set that it is in
    /// by automatic grouping (it already is a set of its own; the engine would rename it `<stem>~2`).
    pub fn remove_member(&mut self, id: &str, item: &str) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        if self.is_packed(item) {
            return Err(PACKED_MOVE.into());
        }
        let Some(v) = self.view_of(id) else {
            return Err(format!("no set {id}"));
        };
        if !v.members.iter().any(|k| k == item) {
            return Err(format!("{item} is not in {}", v.id));
        }
        let moved_in = self.sets_doc.as_ref().is_some_and(|d| {
            entry_of(&d.entries(), &v.id)
                .is_some_and(|e| e.add.iter().any(|f| crate::sets_doc::names_file(f, item)))
        });
        if v.members.len() == 1 && !moved_in {
            return Err(format!("{item} is the only texture in {}", v.id));
        }
        self.leaves_locked(item)?;
        let (i, it) = (v.id.clone(), item.to_string());
        self.edit_docs(&format!("Remove from {i}"), None, true, move |_, s| {
            s.remove_member(&i, &it)
        });
        Ok(())
    }

    /// Note DU1-DU2: drop entry `m` of set `id` that matches no file, as one step: a path leaves `add`/`remove`, a
    /// borrow is dropped, and an entry with no files left is deleted (`delete_set`).
    pub fn remove_missing(&mut self, id: &str, m: &Missing) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(self
                .sets_read_only_reason()
                .unwrap_or_else(|| READ_ONLY.into()));
        }
        let id = self.view_of(id).ok_or_else(|| format!("no set {id}"))?.id;
        match m.clone() {
            Missing::NoFilesLeft => {
                self.delete_set(&id);
            }
            Missing::File(f) => self.edit_docs("Remove from set", None, true, move |_, s| {
                s.drop_entry(&id, &f)
            }),
            // F3a Task 8: a locked slot keeps its borrow, dead or not.
            Missing::Borrow { slot, .. } if self.slot_locked(&id, slot) => {
                return Err(SLOT_LOCKED.into());
            }
            Missing::Borrow { slot, .. } => {
                self.edit_docs("Remove from set", None, true, move |_, s| {
                    s.set_borrow(&id, slot, None)
                })
            }
        }
        Ok(())
    }

    /// Why `sdc-project.toml` takes no edit, when it does not (the set edits' disabled reason).
    pub fn sets_read_only_reason(&self) -> Option<String> {
        self.sets_doc
            .as_ref()
            .and_then(|d| d.error())
            .map(|e| format!("{READ_ONLY}: {e}"))
    }

    /// Choose set `set`'s shader, or with `None` go back to Auto (M6 spec §7). Plans again.
    pub fn set_shader(&mut self, set: &str, shader: Option<Shader>) {
        if !self.sets_editable() {
            return;
        }
        let id = set.to_string();
        let retype = self.untouched_for_shader(set, shader);
        // Note FC1 R5: a sub-type the plan recorded on its own and nobody changed takes the chosen shader's type in
        // this step (the CLI never reads the set's shader, so `sdc.toml` must name it). Back to Auto it goes, and the
        // next settled plan records what the plan picks then.
        for (k, _) in &retype {
            self.history.auto_written.remove(&(k.clone(), "subtype"));
        }
        let pick = move |p: &mut crate::project::ProjectDoc, d: &mut crate::sets_doc::SetsDoc| {
            d.set_shader(&id, shader);
            for (k, new) in &retype {
                match new {
                    Some(v) => p.set_file_value(k, "subtype", v),
                    None => p.clear_file_value(k, "subtype"),
                }
            }
        };
        let Some((item, was, on)) = self.skin_follow(set, shader) else {
            self.edit_docs("Shader", None, true, pick);
            return;
        };
        // Note DU6-DU7: the shader and the diffuse's sub-type (with Note DQ's recipe follow) are one step.
        self.batch_edit("Shader", |m| {
            m.edit_docs("Shader", None, true, pick);
            if on {
                m.edit_subtype("Shader", &item, Some(Subtype::SkinAlbedo));
            } else {
                // The sub-type written before (DQ3 takes the packed output back to `_rmaos`); none was written: the
                // explicit `diffuse` moves the recipe back, then goes, so the sub-type is inferred again.
                let back =
                    serde_json::from_value(serde_json::json!(was)).unwrap_or(Subtype::Diffuse);
                m.edit_subtype("Shader", &item, Some(back));
                if was.is_empty() {
                    m.edit_docs("Shader", None, true, |p, _| {
                        p.clear_file_value(&item, "subtype")
                    });
                }
            }
            m.edit_docs("Shader", None, true, |_, s| {
                let mut skin = s.auto_skin();
                if on {
                    skin.insert(item.clone(), was.clone());
                } else {
                    skin.remove(&item);
                }
                s.set_auto_skin(&skin);
            });
        });
    }

    /// Note FC1 R5: set `set`'s members whose recorded sub-type is untouched under the shader being left and whose
    /// pick under `shader` differs (`None`, Auto: the pick without a shader; the plan re-applies an automatic one);
    /// a locked slot's member too, as the engine's shader re-type ignored locks before FC1 (the lock guards its
    /// encoding). Each with the value to write (`None` under Auto: cleared).
    fn untouched_for_shader(
        &self,
        set: &str,
        shader: Option<Shader>,
    ) -> Vec<(String, Option<serde_json::Value>)> {
        let views = views(self);
        let Some(v) = views.iter().find(|v| same_id(&v.id, set)) else {
            return vec![];
        };
        let now = v.shader.as_ref().map(|p| p.shader);
        let chosen = v
            .shader
            .as_ref()
            .is_some_and(|p| matches!(p.provenance, engine::shader::ShaderOrigin::Set));
        // Auto while already on Auto changes nothing.
        if shader.is_none() && !chosen {
            return vec![];
        }
        let held = |k: &String| {
            self.project
                .as_ref()
                .and_then(|p| p.file_value(k, "subtype"))
        };
        v.members
            .iter()
            .filter(|k| self.untouched_subtype(k, now))
            .filter_map(|k| {
                let new = self.auto_pick(k, shader).map(|st| serde_json::json!(st));
                (new.is_some() && new != held(k))
                    .then(|| (k.clone(), new.filter(|_| shader.is_some())))
            })
            .collect()
    }

    /// Note DU6-DU9: what choosing `shader` for set `set` does to its own diffuse: `(item, its own [files] sub-type
    /// before, true)` to make it `skin_albedo` (Advanced Skin, on cs, a free diffuse whose sub-type is inferred or a
    /// plain `diffuse`), or `(item, the recorded sub-type, false)` to revert one Tessalith made `skin_albedo` and the
    /// user has not changed since. `None`: the shader alone.
    fn skin_follow(&self, set: &str, shader: Option<Shader>) -> Option<(String, String, bool)> {
        let views = views(self);
        let v = views.iter().find(|v| same_id(&v.id, set))?;
        let SlotFill::Own { item } = v.slots.get(&SlotKind::Diffuse)? else {
            return None;
        };
        let p = self.project.as_ref().filter(|p| !p.read_only())?;
        let s = self.sets_doc.as_ref()?;
        // F3a Task 8: a pinned diffuse keeps its sub-type both ways (review issue 2).
        if locked_items(self).contains_key(item) {
            return None;
        }
        let own = p.file_value(item, "subtype");
        if shader == Some(Shader::AdvancedSkin) {
            if self.target != Target::Cs {
                return None;
            }
            let job = self.item(item)?.job.as_ref()?;
            let inferred = job.provenance.get("subtype") == Some(&Provenance::Suggestion);
            let plain = job.profile.subtype == Subtype::Diffuse;
            if job.profile.subtype == Subtype::SkinAlbedo || !(inferred || plain) {
                return None;
            }
            let was = own.as_ref().and_then(|v| v.as_str()).unwrap_or("");
            return Some((item.clone(), was.to_string(), true));
        }
        let was = s.auto_skin().get(item)?.clone();
        (own == Some(serde_json::json!(Subtype::SkinAlbedo))).then(|| (item.clone(), was, false))
    }

    /// F3b D2/D3: rename set `set`'s outputs to base name `name` (`None` restores the names that mirror the sources).
    /// A PBR set on CS writes its PGPatcher `rename` key (V3: a file name; the outputs and the JSON follow the
    /// prefix); any other set writes `output_name`. A re-plan follows; inside `batch_edit` it joins the batch.
    pub fn set_output_name(&mut self, set: &str, name: Option<&str>) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        let Some(v) = self.view_of(set) else {
            return Err(format!("no set {set}"));
        };
        let name = name.map(strip_lead).filter(|n| !n.is_empty());
        if let Some(n) = name {
            check_output_name(n)?;
        }
        let (id, name) = (v.id.clone(), name.map(str::to_string));
        if v.pbr && self.target == Target::Cs {
            let value = name.map(Value::String);
            self.edit_docs("Rename outputs", None, true, move |_, d| {
                d.set_value(&id, "rename", value);
                d.set_output_name(&id, None);
            });
        } else {
            self.edit_docs("Rename outputs", None, true, move |_, d| {
                d.set_output_name(&id, name.as_deref())
            });
        }
        Ok(())
    }

    /// F3b: set `set`'s `output_name`, if it has one (on a CS PBR set it is ignored; the set editor says so).
    pub fn output_name_of(&self, set: &str) -> Option<String> {
        let entries = self.sets_doc.as_ref()?.entries();
        entry_of(&entries, set)?.output_name.clone()
    }

    /// F3b: remove set `set`'s `output_name` only (its PGPatcher `rename` stays).
    pub fn clear_output_name(&mut self, set: &str) {
        if !self.sets_editable() {
            return;
        }
        let id = set.to_string();
        self.edit_docs("Clear output name", None, true, move |_, d| {
            d.set_output_name(&id, None)
        });
    }

    /// Several keys of one set as one undo step and one plan (the Subsurface selector writes two). One key keeps
    /// `set_json_value`'s merge with the edit before it.
    pub fn set_json_values(&mut self, set: &str, edits: Vec<(String, Option<Value>)>) {
        if let [(k, v)] = edits.as_slice() {
            return self.set_json_value(set, k, v.clone());
        }
        if edits.is_empty() || !self.sets_editable() {
            return;
        }
        let label = format!(
            "Change {}",
            edits
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.batch_edit(&label, |m| {
            for (k, v) in edits {
                m.set_json_value(set, &k, v);
            }
        });
    }

    /// A JSON value of set `set`. Material values do not plan; match fields do (P7).
    pub fn set_json_value(&mut self, set: &str, key: &str, v: Option<Value>) {
        if !self.sets_editable() {
            return;
        }
        // DECISIONS AF6: a locked slot's command is changed by nothing, the JSON editor's x on its pill included.
        let slot = key.strip_prefix("slot").and_then(|n| n.parse::<u8>().ok());
        if slot.is_some_and(|n| self.slot_locked(set, n)) {
            return;
        }
        let replan = replans(key);
        let v = match v {
            Some(Value::String(t)) if is_path_key(key) => Some(Value::from(strip_lead(&t))),
            v => v,
        };
        // DECISIONS AN2: a renamed set takes no extra match; the controls show the reason.
        if self.view_of(set).is_some_and(|view| {
            let derived = view.derived_match.as_deref().unwrap_or("");
            extra_match_refusal(&view.settings, derived, key, v.as_ref()).is_some()
        }) {
            return;
        }
        let (s_id, k) = (set.to_string(), key.to_string());
        let merge = Some(format!("{set}:{key}"));
        self.edit_docs(&format!("Change {key}"), merge, replan, move |_, d| {
            d.set_value(&s_id, &k, v)
        });
    }

    /// A slot command (spec §4.2). A path is checked first; the reason comes back when it is refused. One step each
    /// (no merge: F3a Task 8), refused on a locked slot.
    pub fn set_slot_command(
        &mut self,
        set: &str,
        slot: u8,
        cmd: SlotCommand,
    ) -> Result<(), String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        if !(1..=8).contains(&slot) {
            return Err(format!("slot{slot}: there are slots 1 to 8"));
        }
        if self.slot_locked(set, slot) {
            return Err(SLOT_LOCKED.into());
        }
        let key = format!("slot{slot}");
        let v = match cmd {
            SlotCommand::FromSet => None,
            SlotCommand::Clear => Some(Value::from("")),
            SlotCommand::Path(p) => {
                let p = strip_lead(&p);
                check_rel_path(p, &key).map_err(|e| e.0)?;
                if !p.to_ascii_lowercase().ends_with(".dds") {
                    return Err(format!("{key}: the path must end in .dds"));
                }
                Some(Value::from(p.replace('/', "\\")))
            }
        };
        let id = set.to_string();
        self.edit_docs(&format!("Change {key}"), None, false, move |_, d| {
            d.set_value(&id, &key, v)
        });
        Ok(())
    }

    /// Item 17 (DECISIONS AG98): files dropped from Explorer on slot card `slot` of set `set`. The first file, when it
    /// is a texture of the open folder that fills that slot, moves in as the Sets column's move does (`drop_choice`
    /// with Move: Replace / Cancel on a filled slot, refused on a locked one). Nothing is ever copied: a file outside
    /// the folder is refused, and Share or Duplicate are not offered.
    pub fn drop_file_on_slot(
        &mut self,
        set: &str,
        slot: SlotKind,
        paths: &[PathBuf],
    ) -> SlotFileDrop {
        let note = (paths.len() > 1).then_some(ONLY_FIRST_FILE);
        let refused = |why: String| SlotFileDrop {
            item: None,
            outcome: DropOutcome::Refused(why),
            note,
        };
        let Some(first) = paths.first() else {
            return refused("nothing was dropped".into());
        };
        let name = first.file_name().map_or_else(
            || first.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        // Explorer's case can differ from the folder walk's: compare without case, `/` for `\`.
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
        let rel = self.root.as_deref().and_then(|r| {
            let (r, f) = (norm(r), norm(first));
            f.strip_prefix(&format!("{}/", r.trim_end_matches('/')))
                .map(str::to_string)
        });
        let Some(rel) = rel else {
            return refused(format!(
                "{name} is outside the open folder; files are never copied"
            ));
        };
        let Some(it) = self.items.iter().find(|i| i.key.to_lowercase() == rel) else {
            return refused(format!("{name} is not a texture of the open folder"));
        };
        let key = it.key.clone();
        match slot_of(it) {
            // Note AO: a card is a PGPatcher slot; slot 7's takes `_cnr` or `_f`.
            Some(k) if k.slot_number() == slot.slot_number() => {}
            Some(k) => {
                return refused(format!(
                    "{name} goes in the {} slot, not {}",
                    k.label(),
                    slot.label()
                ))
            }
            None => {
                return refused(format!(
                    "{name} fills no slot row; drag it onto the set in the Sets column"
                ))
            }
        }
        let outcome = if self.set_of_item(&key).is_some_and(|f| same_id(&f.id, set)) {
            DropOutcome::Joined
        } else if let Some(why) = self.view_of(set).and_then(|t| {
            // The slot 3/7 clash guard `drop_on_set` applies (AG99).
            slot_conflict(&t.slots.keys().copied().collect::<Vec<_>>(), slot_of(it)?)
        }) {
            DropOutcome::Refused(why)
        } else {
            self.drop_choice(&key, set, DropChoice::Move)
        };
        SlotFileDrop {
            item: Some(key),
            outcome,
            note,
        }
    }

    /// Item 18 (DECISIONS AG99): move the ticked proposals of `fill_proposals` into set `set` as one step and one
    /// plan, each emptying its slot first. A blocked row (a locked slot, …) is never applied. Returns how many moved.
    pub fn fill_from_names(&mut self, set: &str, ticked: &[String]) -> Result<usize, String> {
        if !self.sets_editable() {
            return Err(READ_ONLY.into());
        }
        let v = self.view_of(set).ok_or_else(|| format!("no set {set}"))?;
        let moves: Vec<(String, SlotKind, Option<SlotFill>)> = fill_proposals(self, set)
            .into_iter()
            .filter(|r| r.blocked.is_none() && ticked.contains(&r.item))
            .map(|r| (r.item, r.slot, v.slots.get(&r.slot).cloned()))
            .collect();
        if moves.is_empty() {
            return Err("nothing to fill".into());
        }
        let (id, n) = (v.id.clone(), moves.len());
        self.edit_docs(&format!("Fill {id} from names"), None, true, move |_, s| {
            for (item, slot, existing) in &moves {
                clear_slot(s, &id, Some(*slot), existing);
                s.add_member(&id, item);
            }
        });
        Ok(n)
    }
}

/// Item 17: what a file drop on a slot card did. `item` is the texture it named, when it is one of the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotFileDrop {
    pub item: Option<String>,
    pub outcome: DropOutcome,
    pub note: Option<&'static str>,
}

/// Item 17: several files dropped on one slot.
pub const ONLY_FIRST_FILE: &str = "only the first file was used";

/// Item 18: one row of the Fill from names dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillProposal {
    pub item: String,
    /// The file name.
    pub name: String,
    pub slot: SlotKind,
    /// The file name of what fills that slot now.
    pub replaces: Option<String>,
    /// Why the row cannot be ticked: "locked" for a locked slot, else the move's own refusal.
    pub blocked: Option<String>,
}

/// Item 18 (DECISIONS AG99): the textures of set `set`'s folder that are not its members and whose name is the set's
/// base name plus a tail the engine recognises (its §6.1 suffixes and §4.4 vocabulary, through `base_of`), each with
/// the slot its sub-type fills. By item key; a second file for one slot is blocked.
pub fn fill_proposals(m: &AppModel, set: &str) -> Vec<FillProposal> {
    let views = views(m);
    let Some(v) = views.iter().find(|v| same_id(&v.id, set)) else {
        return vec![];
    };
    // The base name of the diffuse's file (a renamed set keeps its files' names), else of the set's id.
    let src = match v.slots.get(&SlotKind::Diffuse) {
        Some(SlotFill::Own { item }) => {
            let stem = Path::new(item)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            format!("{}/{}", dir_of(item), base_of(&stem))
        }
        _ => v.id.clone(),
    };
    let (dir, base) = (dir_of(&src), src.rsplit('/').next().unwrap_or(&src));
    let mut cands: Vec<&TextureItem> = m
        .items
        .iter()
        .filter(|i| i.kind != ItemKind::Packed && !v.members.contains(&i.key))
        .filter(|i| dir_of(&i.key).eq_ignore_ascii_case(dir))
        .filter(|i| {
            let stem = Path::new(&i.key)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            stem.len() > base.len() && base_of(&stem).eq_ignore_ascii_case(base)
        })
        .collect();
    cands.sort_by(|a, b| a.key.cmp(&b.key));
    let name_of = |k: &str| m.item(k).map_or(k.to_string(), |i| i.name.clone());
    // Grows with each accepted row, so a batch cannot add both halves of a slot 3/7 clash (AG99).
    let mut existing: Vec<SlotKind> = v.slots.keys().copied().collect();
    let mut taken = BTreeSet::new();
    cands
        .into_iter()
        .filter_map(|i| {
            let slot = slot_of(i)?;
            let replaces = match v.slots.get(&slot) {
                Some(SlotFill::Own { item } | SlotFill::Borrowed { item, .. }) => {
                    Some(name_of(item))
                }
                _ => None,
            };
            let blocked = if v.locks.contains(&slot.slot_number()) {
                Some("locked".to_string())
            } else if !taken.insert(slot) {
                Some("another file above takes this slot".to_string())
            } else {
                // The move's own checks, as `drop_choice` makes them (a packed occupant, leaving a locked slot, …).
                slot_conflict(&existing, slot).or_else(|| {
                    m.occupant(v, Some(slot), None)
                        .and_then(|_| m.choice_allowed(&i.key, Some(slot), DropChoice::Move))
                        .err()
                })
            };
            if blocked.is_none() {
                existing.push(slot);
            }
            Some(FillProposal {
                item: i.key.clone(),
                name: i.name.clone(),
                slot,
                replaces,
                blocked,
            })
        })
        .collect()
}

/// The engine key of the output `item` writes: its file, or its set and recipe for a packed output. `None` for an
/// item that writes no output of its own.
pub fn item_output_key(views: &[SetView], item: &TextureItem) -> Option<OutputKey> {
    slot_of(item)?;
    if item.kind != ItemKind::Packed {
        return Some(OutputKey::File(item.key.clone()));
    }
    let suffix = recipe_suffix(item)?;
    let set = views
        .iter()
        .find(|v| {
            v.slots
                .values()
                .any(|f| matches!(f, SlotFill::Own { item: i } if *i == item.key))
        })?
        .id
        .clone();
    Some(OutputKey::Packed { set, suffix })
}
