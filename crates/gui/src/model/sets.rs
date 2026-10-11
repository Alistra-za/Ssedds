//! The user's texture sets (M3b spec §3, §6.1): views for the Sets column and set editor, shared-slot ownership,
//! the engine inputs (`set_edits`, `output_paths`) and the drop rules. No egui types.
use super::history::{sha256_of, CreatedFile};
use super::items::{ItemKind, ItemStatus, TextureItem};
use super::AppModel;
use crate::pgp::prefix::{check_rel_path, derive_match_diffuse, output_rel, prefix_of};
use crate::pgp::settings::Settings;
use crate::sets_doc::{SetEntry, SetsDoc};
use engine::config::{Subtype, Target};
use engine::group::{split_suffix, split_vocab, VocabRole};
use engine::overrides::path_key_matches;
use engine::sets::{fold_id, same_id, set_id, OutputKey, SetEdit};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Diffuse,
    Normal,
    Emissive,
    Height,
    Rmaos,
    Coat,
    Fuzz,
    Subsurface,
}

impl SlotKind {
    /// The slot cards' order (spec §4.1).
    pub const ALL: [SlotKind; 8] = [
        SlotKind::Diffuse,
        SlotKind::Normal,
        SlotKind::Rmaos,
        SlotKind::Height,
        SlotKind::Emissive,
        SlotKind::Subsurface,
        SlotKind::Coat,
        SlotKind::Fuzz,
    ];
    /// PGPatcher's 1-based `slotN` (DECISIONS P6).
    pub fn slot_number(self) -> u8 {
        match self {
            SlotKind::Diffuse => 1,
            SlotKind::Normal => 2,
            SlotKind::Emissive => 3,
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
            SlotKind::Height => "p",
            SlotKind::Rmaos => "rmaos",
            SlotKind::Coat => "cnr",
            SlotKind::Fuzz => "f",
            SlotKind::Subsurface => "s",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SlotKind::Diffuse => "Diffuse",
            SlotKind::Normal => "Normal _n",
            SlotKind::Emissive => "Emissive _g",
            SlotKind::Height => "Height _p",
            SlotKind::Rmaos => "RMAOS",
            SlotKind::Coat => "Coat _cnr",
            SlotKind::Fuzz => "Fuzz _f",
            SlotKind::Subsurface => "Subsurface _s",
        }
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct SetView {
    /// The engine key (`""` for a created set with no members yet).
    pub key: String,
    pub id: String,
    pub label: String,
    /// Item keys of every member, packed outputs and raw sources included.
    pub members: Vec<String>,
    pub slots: BTreeMap<SlotKind, SlotFill>,
    /// The diffuse resolves to `pbr_albedo` (DECISIONS P5).
    pub pbr: bool,
    pub created: bool,
    /// `add`/`remove` entries of `sdc-project.toml` that match no file (Review focus 1).
    pub missing: Vec<String>,
    /// Slots whose finished output this set owns while another set borrows it.
    pub owns: Vec<SlotKind>,
    pub derived_match: Option<String>,
    /// The output prefix, or why there is none (shown, and an export block for a PBR set on CS).
    pub prefix: Option<String>,
    pub prefix_error: Option<String>,
    /// The set's own JSON settings.
    pub settings: Settings,
}

impl SetView {
    fn new(key: String, id: String, members: Vec<String>) -> SetView {
        SetView {
            key,
            label: id.clone(),
            id,
            members,
            slots: BTreeMap::new(),
            pbr: false,
            created: false,
            missing: vec![],
            owns: vec![],
            derived_match: None,
            prefix: None,
            prefix_error: None,
            settings: Settings::new(),
        }
    }
}

/// The slot a texture fills, by its resolved sub-type. Raw packing sources and items without a job have none.
pub fn slot_of(item: &TextureItem) -> Option<SlotKind> {
    if matches!(item.status, ItemStatus::PackedInto(_)) {
        return None;
    }
    let st = item.job.as_ref()?.profile.subtype;
    Some(match st {
        Subtype::PbrAlbedo | Subtype::Diffuse | Subtype::LandscapeDiffuse | Subtype::SkinAlbedo => {
            SlotKind::Diffuse
        }
        Subtype::PbrNormal | Subtype::Normal | Subtype::LandscapeNormal | Subtype::Msn => {
            SlotKind::Normal
        }
        Subtype::PbrEmissive | Subtype::Glow => SlotKind::Emissive,
        Subtype::Height => SlotKind::Height,
        Subtype::Rmaos | Subtype::SkinRfaos => SlotKind::Rmaos,
        Subtype::CoatNormal => SlotKind::Coat,
        Subtype::PbrFuzz => SlotKind::Fuzz,
        Subtype::PbrSubsurface => SlotKind::Subsurface,
        _ => return None,
    })
}

/// Why `incoming` cannot join a set holding `existing`, if it cannot (spec §4.1: `_cnr` and `_f` share slot 7).
pub fn slot_conflict(existing: &[SlotKind], incoming: SlotKind) -> Option<String> {
    let clash = match incoming {
        SlotKind::Coat => existing.contains(&SlotKind::Fuzz),
        SlotKind::Fuzz => existing.contains(&SlotKind::Coat),
        _ => false,
    };
    clash.then(|| {
        "a set cannot hold both a coat normal (_cnr) and a fuzz map (_f): both use slot 7"
            .to_string()
    })
}

fn entry_of<'a>(entries: &'a [SetEntry], id: &str) -> Option<&'a SetEntry> {
    entries.iter().find(|e| same_id(&e.id, id))
}

fn dir_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(d, _)| d)
}

/// A source stem without its exporter or Skyrim suffix (`rock01_roughness` → `rock01`, `rock01_n` → `rock01`).
fn base_of(stem: &str) -> String {
    split_vocab(stem).map_or_else(|| split_suffix(stem).0, |(b, _)| b)
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

/// Every set, in display order, with its slots, ownership and prefix.
pub fn set_views(m: &AppModel) -> Vec<SetView> {
    let root = m.export_root();
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    let order = m.sets_doc.as_ref().map(|d| d.order()).unwrap_or_default();

    let mut views: Vec<SetView> = m
        .sets
        .iter()
        .map(|s| {
            let mut v = SetView::new(s.key.clone(), set_id(&root, &s.key), s.members.clone());
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
        if v.key.is_empty() && !e.created {
            v.missing.push(NO_FILES_LEFT.into());
        }
        for f in e.add.iter().chain(&e.remove) {
            if !m.items.iter().any(|i| path_key_matches(f, &i.key)) {
                v.missing.push(f.clone());
            }
        }
        for (slot, from) in &e.borrow {
            let Some((_, src)) = own_slots.iter().find(|(id, _)| same_id(id, from)) else {
                v.missing.push(format!("slot{slot} from {from}"));
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
                v.missing
                    .push(format!("slot{slot} from {from} (it has none)"));
            }
        }
    }
    // PBR, derived match, prefix.
    for v in &mut views {
        let diffuse = match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item: k } | SlotFill::Borrowed { item: k, .. }) => m.item(k),
            _ => None,
        };
        v.pbr = diffuse
            .and_then(|i| i.job.as_ref())
            .is_some_and(|j| j.profile.subtype == Subtype::PbrAlbedo);
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
    for e in &entries {
        if let Some(from) = &e.renamed_from {
            edits.push(SetEdit::Rename {
                from: from.clone(),
                to: e.id.clone(),
            });
        }
    }
    for e in &entries {
        for f in &e.add {
            edits.push(SetEdit::Move {
                file: f.clone(),
                to: e.id.clone(),
            });
        }
    }
    for e in &entries {
        for f in &e.remove {
            edits.push(SetEdit::Remove { file: f.clone() });
        }
    }
    let mut paths = BTreeMap::new();
    if m.target != Target::Cs {
        return (edits, paths);
    }
    let views = set_views(m);
    let candidates = output_candidates(m, &views);
    // Mirrored outputs: textures with no explicit path, now or in the plan on screen, write their job key. An owned
    // path on one is held back too (the engine would skip both as IN-DUPLICATE and the plan would not settle);
    // `path_collisions` reports it.
    let explicit = |k: &OutputKey| {
        candidates.iter().any(|(_, c, _)| c == k)
            || m.last_inputs
                .as_ref()
                .is_some_and(|(_, p)| p.contains_key(k))
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
    for (_, key, rel) in candidates {
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
        let rel = output_rel(prefix, slot.suffix());
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
    fn view_of(&self, id: &str) -> Option<SetView> {
        set_views(self).into_iter().find(|v| same_id(&v.id, id))
    }

    fn set_of_item(&self, item: &str) -> Option<SetView> {
        set_views(self)
            .into_iter()
            .find(|v| v.members.iter().any(|k| k == item))
    }

    fn sets_editable(&self) -> bool {
        self.sets_doc.as_ref().is_some_and(|d| !d.read_only())
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
                    .into_iter()
                    .filter_map(|e| e.renamed_from.map(|f| fold_id(&f))),
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
    /// `None` when nothing was done (read-only `sdc-project.toml`, or a packed map, which follows its raw maps).
    pub fn drop_new_set(&mut self, item: &str) -> Option<String> {
        if !self.sets_editable() || self.is_packed(item) {
            return None;
        }
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
        Some(id)
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
        let mut dst = name(&set_base);
        let mut n = 2;
        while dst.exists() {
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
    pub fn reorder_set(&mut self, id: &str, to: usize) {
        if !self.sets_editable() {
            return;
        }
        let mut ids: Vec<String> = set_views(self).into_iter().map(|v| v.id).collect();
        let Some(from) = ids.iter().position(|i| same_id(i, id)) else {
            return;
        };
        let moved = ids.remove(from);
        ids.insert(to.min(ids.len()), moved);
        self.edit_docs("Reorder sets", None, true, move |_, s| s.set_order(&ids));
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
        self.edit_docs(&format!("Rename {a}"), None, true, move |_, s| {
            s.rename_set(&a, &b)
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
            entry_of(&d.entries(), &v.id).is_some_and(|e| e.add.iter().any(|f| same_id(f, item)))
        });
        if v.members.len() == 1 && !moved_in {
            return Err(format!("{item} is the only texture in {}", v.id));
        }
        let (i, it) = (v.id.clone(), item.to_string());
        self.edit_docs(&format!("Remove from {i}"), None, true, move |_, s| {
            s.remove_member(&i, &it)
        });
        Ok(())
    }

    /// A JSON value of set `set`. Material values do not plan; match fields do (P7).
    pub fn set_json_value(&mut self, set: &str, key: &str, v: Option<Value>) {
        if !self.sets_editable() {
            return;
        }
        let replan = replans(key);
        let (s_id, k) = (set.to_string(), key.to_string());
        let merge = Some(format!("{set}:{key}"));
        self.edit_docs(&format!("Change {key}"), merge, replan, move |_, d| {
            d.set_value(&s_id, &k, v)
        });
    }

    /// A slot command (spec §4.2). A path is checked first; the reason comes back when it is refused.
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
        let key = format!("slot{slot}");
        let v = match cmd {
            SlotCommand::FromSet => None,
            SlotCommand::Clear => Some(Value::from("")),
            SlotCommand::Path(p) => {
                check_rel_path(&p, &key).map_err(|e| e.0)?;
                if !p.trim().to_ascii_lowercase().ends_with(".dds") {
                    return Err(format!("{key}: the path must end in .dds"));
                }
                Some(Value::from(p.trim().replace('/', "\\")))
            }
        };
        self.set_json_value(set, &key, v);
        Ok(())
    }
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
