//! M6 Track F3a (design `2026-10-13-sdc-m6-f3a-design.md`): the multi-selection, the bulk edit plan and its
//! commands. No egui types.
use super::fields::{
    all_fields, field_hidden, field_view, filter_option_lock, views_of, FieldView, EDGE_FIELD,
    FIELDS, FILTER_FIELD, LEVELS_FIELD,
};
use super::sets::{
    locked_items, member_game_path, slot_gives, slot_of, SlotCommand, SlotKind, SLOT_LOCKED,
};
use super::{AppModel, Dialog, Selection};
use engine::config::{MipFilter, Subtype};
use engine::policy::ResolvedJob;
use engine::sets::same_id;
use serde_json::Value;

/// F3a D1: every selected key of one kind, the active one included, in the order they were added. It is in effect
/// only while it holds two or more keys and the active entry (`AppModel::selection`) is one of them; otherwise the
/// selection is the single active entry, as before F3a.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultiSelection {
    pub keys: Vec<String>,
    /// The keys are set keys (else item keys).
    pub sets: bool,
}

/// What the bulk panel keeps between frames: a new selection resets it, a plan does not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkState {
    /// F3a D3: the map-kind chip over the selected sets; `None` is All.
    pub kind: Option<SlotKind>,
    /// The last apply's result line (F3a D2: skips are reported).
    pub report: Option<String>,
    cache: Option<BulkCache>,
}

type CacheInputs = (
    MultiSelection,
    Selection,
    Option<SlotKind>,
    u64,
    u64,
    u64,
    String,
);

/// What `bulk_view` computed and what it was computed from: the multi-selection and active entry, the picked kind,
/// the view-cache revisions (the sets document's too: a slot lock plans nothing) and the target. Equal inputs reuse the result (N=3000 would otherwise serialise 3000
/// profiles a frame).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BulkCache {
    inputs: CacheInputs,
    view: std::sync::Arc<BulkView>,
    computes: usize,
}

/// The bulk panel's targets and rows.
#[derive(Debug, PartialEq, Eq)]
pub struct BulkView {
    pub keys: Vec<String>,
    pub rows: Vec<BulkField>,
}

impl AppModel {
    /// The active entry's key, when it is of the kind asked for.
    fn active_of(&self, sets: bool) -> Option<&str> {
        match (&self.selection, sets) {
            (Selection::Item(k), false) | (Selection::Set(k), true) => Some(k),
            _ => None,
        }
    }

    fn multi_in_effect(&self) -> bool {
        self.multi.keys.len() >= 2
            && self
                .active_of(self.multi.sets)
                .is_some_and(|a| self.multi.keys.iter().any(|k| k == a))
    }

    fn selected(&self, sets: bool) -> Vec<String> {
        if self.multi_in_effect() && self.multi.sets == sets {
            return self.multi.keys.clone();
        }
        self.active_of(sets)
            .map(|k| vec![k.to_string()])
            .unwrap_or_default()
    }

    /// The selected textures (item keys) in the order they were added; one or none outside a multi-selection.
    pub fn selected_items(&self) -> Vec<String> {
        self.selected(false)
    }

    /// The selected sets (set keys) in the order they were added.
    pub fn selected_sets(&self) -> Vec<String> {
        self.selected(true)
    }

    /// Whether `key` (a set key when `sets`) is selected: a tile's or a pill's highlight.
    pub fn is_selected(&self, key: &str, sets: bool) -> bool {
        if self.multi_in_effect() && self.multi.sets == sets {
            self.multi.keys.iter().any(|k| k == key)
        } else {
            self.active_of(sets) == Some(key)
        }
    }

    /// Two or more textures, or two or more sets, are selected: the centre shows the bulk panel.
    pub fn in_bulk(&self) -> bool {
        self.multi_in_effect()
    }

    fn visible_keys(&self) -> Vec<String> {
        self.visible_items()
            .into_iter()
            .map(|i| self.items[i].key.clone())
            .collect()
    }

    fn set_order(&self) -> Vec<String> {
        super::sets::views(self)
            .iter()
            .filter(|v| !v.key.is_empty())
            .map(|v| v.key.clone())
            .collect()
    }

    /// F3a D1: a click on texture `key`. Plain selects it alone; Ctrl toggles it; Shift selects the range from the
    /// active texture to it in the displayed order (search and filters applied). Shift with no active texture, or
    /// with the active one hidden, is a plain click.
    pub fn click_item(&mut self, key: &str, ctrl: bool, shift: bool) {
        let order = self.visible_keys();
        self.click(key, ctrl, shift, false, &order);
    }

    /// F3a D1: the same over the Sets column, in its display order (sets have no filter).
    pub fn click_set(&mut self, key: &str, ctrl: bool, shift: bool) {
        let order = self.set_order();
        self.click(key, ctrl, shift, true, &order);
    }

    fn click(&mut self, key: &str, ctrl: bool, shift: bool, sets: bool, order: &[String]) {
        let active = self.active_of(sets).map(str::to_string);
        let range = match (&active, shift) {
            (Some(a), true) => {
                let from = order.iter().position(|k| k == a);
                let to = order.iter().position(|k| k == key);
                from.zip(to)
                    .map(|(x, y)| order[x.min(y)..=x.max(y)].to_vec())
            }
            _ => None,
        };
        if let Some(keys) = range {
            return self.set_selection(keys, active, sets);
        }
        // A plain click, or a modifier click on the other kind: that entry alone.
        let Some(a) = active.filter(|_| ctrl) else {
            return if sets {
                self.select_set(key)
            } else {
                self.select_item(key)
            };
        };
        let mut keys = self.selected(sets);
        let next = match keys.iter().position(|k| k == key) {
            Some(i) => {
                keys.remove(i);
                if a == key {
                    keys.last().cloned()
                } else {
                    Some(a)
                }
            }
            None => {
                keys.push(key.to_string());
                Some(key.to_string())
            }
        };
        self.set_selection(keys, next, sets);
    }

    /// Select `keys` with `active` as the active entry; fewer than two keys is a single selection.
    fn put_selection(&mut self, keys: Vec<String>, active: Option<String>, sets: bool) {
        self.selection = match active {
            Some(k) if sets => Selection::Set(k),
            Some(k) => Selection::Item(k),
            None => Selection::None,
        };
        self.multi = MultiSelection { keys, sets };
        if self.multi.keys.len() < 2 {
            self.multi.keys.clear();
        }
    }

    /// `put_selection` for a new selection: the bulk panel starts over (All, no report).
    pub(super) fn set_selection(&mut self, keys: Vec<String>, active: Option<String>, sets: bool) {
        self.put_selection(keys, active, sets);
        self.bulk = BulkState::default();
    }

    /// F3a D1 Ctrl+A: every visible texture (search and filters applied), or every set when a set is the active
    /// entry. The active entry stays when it is among them; else the first becomes active.
    pub fn select_all_visible(&mut self) {
        let sets = matches!(self.selection, Selection::Set(_));
        let keys = if sets {
            self.set_order()
        } else {
            self.visible_keys()
        };
        let active = self
            .active_of(sets)
            .filter(|a| keys.iter().any(|k| k == a))
            .map(str::to_string)
            .or_else(|| keys.first().cloned());
        self.set_selection(keys, active, sets);
    }

    /// F3a D1 Esc.
    pub fn clear_selection(&mut self) {
        self.set_selection(vec![], None, false);
    }

    /// F3a D1: "12 selected", or "12 selected (4 hidden by filter)", for the column of that kind while two or
    /// more are selected there. Sets have no filter.
    pub fn selection_summary(&self, sets: bool) -> Option<String> {
        let keys = self.selected(sets);
        if keys.len() < 2 {
            return None;
        }
        let hidden = if sets {
            0
        } else {
            let shown: std::collections::HashSet<String> =
                self.visible_keys().into_iter().collect();
            keys.iter().filter(|k| !shown.contains(*k)).count()
        };
        Some(match hidden {
            0 => format!("{} selected", keys.len()),
            h => format!("{} selected ({h} hidden by filter)", keys.len()),
        })
    }

    /// After a plan (F3a D1, design §2.4): keys whose item or set is gone leave the multi-selection, and a gone
    /// active key hands over to the last remaining one. The chip and the report are kept.
    pub(super) fn prune_selection(&mut self) {
        if !self.multi_in_effect() {
            self.multi.keys.clear();
            return;
        }
        let sets = self.multi.sets;
        let live: Vec<String> = self
            .multi
            .keys
            .iter()
            .filter(|k| {
                if sets {
                    self.sets.iter().any(|s| &s.key == *k)
                } else {
                    self.item(k).is_some()
                }
            })
            .cloned()
            .collect();
        let active = self
            .active_of(sets)
            .filter(|a| live.iter().any(|k| k == a))
            .map(str::to_string)
            .or_else(|| live.last().cloned());
        self.put_selection(live, active, sets);
    }
}

/// Why a texture without a job takes no write.
pub const NO_SETTINGS: &str = "has no settings of its own (packed, skipped or failed)";
const INVALID: &str = "not a valid value for this setting";
const CUTOUT_ONLY: &str = "used only by cutout and cutout + blend";

/// What a bulk edit does to one field.
#[derive(Debug, Clone, PartialEq)]
pub enum BulkOp {
    Set(Value),
    Clear,
}

/// What a bulk edit does to one texture (F3a D5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Write,
    Skip(String),
}

/// The bulk panel's rows, in the single panel's order: the Mipmaps rows, then `FIELDS` (design 3.2).
pub fn panel_fields() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![LEVELS_FIELD, FILTER_FIELD, EDGE_FIELD];
    v.extend(FIELDS);
    v
}

/// A field's row label, or the field id for one no panel lists.
pub fn field_label(field: &str) -> &str {
    all_fields()
        .into_iter()
        .find(|(f, _)| *f == field)
        .map_or(field, |(_, l)| l)
}

/// The one rule (F3a D2, Note Z2): why a write of `field` skips the texture whose job and view these are, or `None`
/// when it reaches it. `plan_bulk_edit` and `bulk_fields` both call it, so the panel and the apply cannot drift.
/// `op` is `None` for the panel's value-independent counts (locks apply, no value is checked). A Clear is not
/// blocked by a field lock (R6): dropping a stale `[files]` entry is harmless and the only way to remove it. A
/// texture filling a locked slot (`slot_locked`, F3a Task 8) takes neither.
fn skip_of(
    job: Option<&ResolvedJob>,
    view: Option<&FieldView>,
    field: &str,
    op: Option<&BulkOp>,
    slot_locked: bool,
) -> Option<String> {
    if slot_locked {
        return Some(SLOT_LOCKED.into());
    }
    let Some(job) = job else {
        return Some(NO_SETTINGS.into());
    };
    if field_hidden(&job.profile, field) {
        return Some(CUTOUT_ONLY.into());
    }
    let Some(view) = view else {
        return Some(format!("{field} is not a texture setting"));
    };
    if let Some(lock) = view.lock {
        if op != Some(&BulkOp::Clear) {
            return Some(lock.into());
        }
    }
    if let Some(BulkOp::Set(v)) = op {
        // R7: the single-item path writes nothing for a value TOML cannot hold, and the pickers only offer
        // parsable sub-types and filters; an unparsable one is reported, not written.
        let bad = v.is_null()
            || (field == "subtype" && serde_json::from_value::<Subtype>(v.clone()).is_err())
            || (field == "mips.filter" && serde_json::from_value::<MipFilter>(v.clone()).is_err());
        if bad {
            return Some(INVALID.into());
        }
        let barred = serde_json::from_value::<MipFilter>(v.clone())
            .ok()
            .filter(|_| field == "mips.filter")
            .and_then(|f| filter_option_lock(&job.profile, f));
        if let Some(lock) = barred {
            return Some(lock.into());
        }
    }
    None
}

/// F3a D5: what a bulk edit of `field` does to each of `keys`, in order. Pure: the apply writes exactly this.
pub fn plan_bulk_edit(
    m: &AppModel,
    keys: &[String],
    field: &str,
    op: &BulkOp,
) -> Vec<(String, Change)> {
    let locked = locked_items(m);
    keys.iter()
        .map(|k| {
            let job = m.item(k).and_then(|i| i.job.as_ref());
            let view = job.and_then(|j| field_view(m.target, j, field));
            let change = skip_of(job, view.as_ref(), field, Some(op), locked.contains(k))
                .map_or(Change::Write, Change::Skip);
            (k.clone(), change)
        })
        .collect()
}

/// One row of the bulk panel (F3a D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkField {
    pub field: &'static str,
    pub label: &'static str,
    /// The value every texture the field applies to resolves to; `None` when they differ (mixed) or none applies.
    pub value: Option<Value>,
    /// Textures a write reaches.
    pub applies: usize,
    /// Textures it applies to whose own `[files]` entry sets it (the row's reset shows when > 0).
    pub edited: usize,
    /// (reason, count) of the textures a write skips, in first-seen order.
    pub skipped: Vec<(String, usize)>,
    pub total: usize,
}

impl BulkField {
    /// The note column: "— mixed", "applies to K of N", both, or nothing.
    pub fn note(&self) -> String {
        let mut parts = vec![];
        if self.value.is_none() && self.applies > 0 {
            parts.push("— mixed".to_string());
        }
        if self.applies < self.total {
            parts.push(format!("applies to {} of {}", self.applies, self.total));
        }
        parts.join(" · ")
    }

    /// The note's hover: one "locked on M of N: <reason>" line per reason.
    pub fn skipped_text(&self) -> String {
        self.skipped
            .iter()
            .map(|(r, n)| format!("locked on {n} of {}: {r}", self.total))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The bulk panel's rows over `keys` (F3a D2). Counts are the value-independent ones; the filter
/// lock of a picked value shows in the report. One profile serialisation per texture; `AppModel::bulk_view` caches.
pub fn bulk_fields(m: &AppModel, keys: &[String]) -> Vec<BulkField> {
    let fields = panel_fields();
    let mut rows: Vec<BulkField> = fields
        .iter()
        .map(|&(field, label)| BulkField {
            field,
            label,
            value: None,
            applies: 0,
            edited: 0,
            skipped: vec![],
            total: keys.len(),
        })
        .collect();
    let mut values: Vec<Vec<Value>> = vec![vec![]; fields.len()];
    let locked = locked_items(m);
    for k in keys {
        let job = m.item(k).and_then(|i| i.job.as_ref());
        // One profile serialisation per texture, not one per row.
        let views = job.map_or_else(Vec::new, |j| views_of(&fields, m.target, j));
        for (n, row) in rows.iter_mut().enumerate() {
            let view = views.get(n);
            match skip_of(job, view, row.field, None, locked.contains(k)) {
                Some(r) => match row.skipped.iter_mut().find(|e| e.0 == r) {
                    Some(e) => e.1 += 1,
                    None => row.skipped.push((r, 1)),
                },
                None => {
                    row.applies += 1;
                    if let Some(v) = view {
                        if !values[n].contains(&v.value) {
                            values[n].push(v.value.clone());
                        }
                        row.edited += usize::from(v.edited);
                    }
                }
            }
        }
    }
    for (row, mut vals) in rows.iter_mut().zip(values) {
        row.value = if vals.len() == 1 { vals.pop() } else { None };
    }
    rows
}

/// The members of sets `set_keys` (in that order, then member order) whose slot is `kind`; `None` is every member.
pub fn members_of(m: &AppModel, set_keys: &[String], kind: Option<SlotKind>) -> Vec<String> {
    let views = super::sets::views(m);
    let mut out: Vec<String> = vec![];
    for s in set_keys {
        let Some(v) = views.iter().find(|v| &v.key == s) else {
            continue;
        };
        for k in &v.members {
            let fits = kind.is_none() || m.item(k).and_then(super::sets::slot_of) == kind;
            if fits && !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// "1 texture", "3 textures".
fn textures(n: usize) -> String {
    format!("{n} texture{}", if n == 1 { "" } else { "s" })
}

/// One ticked field of a copy: its id, the value, and the plan over the targets.
type CopyWrite = (String, Value, Vec<(String, Change)>);

/// "Format: written to 9 of 12; locked on 3 of 12: BC1 only".
fn report(label: &str, plan: &[(String, Change)]) -> String {
    let n = plan.len();
    let written = plan.iter().filter(|(_, c)| *c == Change::Write).count();
    let mut skipped: Vec<(&str, usize)> = vec![];
    for (_, c) in plan {
        if let Change::Skip(r) = c {
            match skipped.iter_mut().find(|e| e.0 == r.as_str()) {
                Some(e) => e.1 += 1,
                None => skipped.push((r.as_str(), 1)),
            }
        }
    }
    let mut s = format!("{label}: written to {written} of {n}");
    for (r, k) in skipped {
        s.push_str(&format!("; locked on {k} of {n}: {r}"));
    }
    s
}

impl AppModel {
    fn toml_writable(&self) -> bool {
        self.project.as_ref().is_some_and(|p| !p.read_only())
    }

    /// F3a D2/D5: write (or clear) `field` on every texture of `keys` it applies to, through the single-texture
    /// path (a Sub-type runs `edit_subtype`), as one step and one plan. The selection is kept.
    pub fn apply_bulk_edit(&mut self, keys: &[String], field: &str, op: BulkOp) {
        if !self.toml_writable() {
            return;
        }
        let plan = plan_bulk_edit(self, keys, field, &op);
        let label = field_label(field);
        let verb = if op == BulkOp::Clear {
            "Reset"
        } else {
            "Change"
        };
        let written = plan.iter().filter(|(_, c)| *c == Change::Write).count();
        let changed = self.batch_edit(&format!("{verb} {label} on {}", textures(written)), |m| {
            for (k, c) in &plan {
                if *c != Change::Write {
                    continue;
                }
                match &op {
                    BulkOp::Set(v) => m.set_field(k, field, v.clone()),
                    BulkOp::Clear => m.clear_field(k, field),
                }
            }
        });
        // Writes that changed nothing (same values) report nothing; all-skipped plans still say why.
        if changed || written == 0 || plan.iter().any(|(_, c)| matches!(c, Change::Skip(_))) {
            self.bulk.report = Some(report(label, &plan));
        }
    }

    /// F3a D4: the copy checklist for the active texture: every field of `all_fields` its `[files]` entry sets,
    /// ticked. Nothing opens without another selected texture or a field to copy.
    pub fn open_copy_settings(&mut self) {
        let Selection::Item(from) = self.selection.clone() else {
            return;
        };
        let to: Vec<String> = self
            .selected_items()
            .into_iter()
            .filter(|k| *k != from)
            .collect();
        let Some(p) = self.project.as_ref() else {
            return;
        };
        let fields: Vec<(String, bool)> = all_fields()
            .into_iter()
            .filter(|(f, _)| p.file_value(&from, f).is_some())
            .map(|(f, _)| (f.to_string(), true))
            .collect();
        if to.is_empty() || fields.is_empty() {
            return;
        }
        self.dialog = Some(Dialog::CopySettings { from, to, fields });
    }

    /// F3a D4: write each of `fields`, valued as in `from`'s `[files]` entry, to every texture of `to` it applies
    /// to, as one step (Note Z5).
    pub fn apply_copy_settings(&mut self, from: &str, to: &[String], fields: &[String]) {
        self.dialog = None;
        if !self.toml_writable() {
            return;
        }
        let Some(p) = self.project.as_ref() else {
            return;
        };
        let writes: Vec<CopyWrite> = fields
            .iter()
            .filter_map(|f| {
                let v = p.file_value(from, f)?;
                let plan = plan_bulk_edit(self, to, f, &BulkOp::Set(v.clone()));
                Some((f.clone(), v, plan))
            })
            .collect();
        let name = self.item(from).map_or(from.to_string(), |i| i.name.clone());
        self.batch_edit(&format!("Copy settings from {name}"), |m| {
            for (f, v, plan) in &writes {
                for (k, c) in plan {
                    if *c == Change::Write {
                        m.set_field(k, f, v.clone());
                    }
                }
            }
        });
        self.bulk.report = Some(
            writes
                .iter()
                .map(|(f, _, plan)| report(field_label(f), plan))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    /// What Reset all clears: the selected textures, or every member of the selected sets.
    fn reset_targets(&self) -> Vec<String> {
        let sets = self.selected_sets();
        if sets.is_empty() {
            self.selected_items()
        } else {
            members_of(self, &sets, None)
        }
    }

    /// F3a D4: the confirm before Reset all.
    pub fn ask_reset_all(&mut self) {
        let keys = self.reset_targets();
        if !keys.is_empty() {
            self.dialog = Some(Dialog::ConfirmResetAll(keys));
        }
    }

    /// F3a D4 (Note Z6, R3): remove each texture's `[files]` settings as one step, keeping its `allow` (acknowledged
    /// warnings are decisions, not settings). A `[files]` sub-type is cleared first through `clear_field`, so PBR
    /// follow-on members revert as for a single reset.
    pub fn reset_all_settings(&mut self, keys: &[String]) {
        self.dialog = None;
        if !self.toml_writable() {
            return;
        }
        let n = keys.len();
        let label = format!("Reset all settings on {}", textures(n));
        let changed = self.batch_edit(&label, |m| {
            for k in keys {
                if m.project
                    .as_ref()
                    .is_some_and(|p| p.file_value(k, "subtype").is_some())
                {
                    m.clear_field(k, "subtype");
                }
                let k = k.clone();
                m.edit("Reset all settings", move |p| {
                    let allow = p.allow(&k);
                    p.remove_file_entry(&k);
                    p.set_allow(&k, &allow);
                });
            }
        });
        if changed {
            self.bulk.report = Some(label);
        }
    }
}

/// F3a D3: one chip of the kind row over selected sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindChip {
    /// `None` is All.
    pub kind: Option<SlotKind>,
    pub label: String,
    pub members: Vec<String>,
    /// How many of the selected sets have a member of this kind ("applies to K of N sets").
    pub sets_with: usize,
}

/// F3a D3: All, then one chip per slot kind present among the members of `set_keys`, in `SlotKind::ALL` order.
/// Members without a slot (raw packing sources, sub-types without a card) are under All only.
pub fn kind_chips(m: &AppModel, set_keys: &[String]) -> Vec<KindChip> {
    let views = super::sets::views(m);
    let mut seen = std::collections::HashSet::new();
    let mut all: Vec<String> = vec![];
    let mut by_kind: Vec<(SlotKind, Vec<String>, usize)> =
        SlotKind::ALL.into_iter().map(|k| (k, vec![], 0)).collect();
    for s in set_keys {
        let Some(v) = views.iter().find(|v| &v.key == s) else {
            continue;
        };
        let mut hit = vec![false; by_kind.len()];
        for k in &v.members {
            if !seen.insert(k.clone()) {
                continue;
            }
            all.push(k.clone());
            if let Some(slot) = m.item(k).and_then(super::sets::slot_of) {
                if let Some(i) = by_kind.iter().position(|e| e.0 == slot) {
                    by_kind[i].1.push(k.clone());
                    hit[i] = true;
                }
            }
        }
        for (i, h) in hit.into_iter().enumerate() {
            by_kind[i].2 += usize::from(h);
        }
    }
    let mut chips = vec![KindChip {
        kind: None,
        label: "All".into(),
        members: all,
        sets_with: set_keys.len(),
    }];
    chips.extend(by_kind.into_iter().filter(|e| !e.1.is_empty()).map(
        |(kind, members, sets_with)| KindChip {
            kind: Some(kind),
            label: kind.label().into(),
            members,
            sets_with,
        },
    ));
    chips
}

/// F3a D4: what a right-click menu entry does. F3b appends its own entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    EditSettings,
    CopyFromActive,
    ResetAll,
    SelectMembersOfKind(Option<SlotKind>),
    /// F3a Task 8: a Slot commands window command on slot `n` of set `set` (an id), through the same model call.
    Slot {
        set: String,
        n: u8,
        cmd: SlotCommand,
    },
    /// "Path…": the UI opens the Slot commands window on that slot to type it; `run_menu` does nothing.
    SlotPath {
        set: String,
        n: u8,
    },
    /// The "From another set" submenu; its `SlotLink` entries follow it in the list.
    FromAnotherSet,
    /// Slot `n` of `set` takes `item`'s game path (`member_game_path`), as Path would write it.
    SlotLink {
        set: String,
        n: u8,
        item: String,
    },
    SlotLock {
        set: String,
        n: u8,
        on: bool,
    },
}

/// One entry of a right-click menu; `disabled` is the hover reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub label: String,
    pub disabled: Option<String>,
}

impl AppModel {
    /// Why `sdc.toml` takes no edit, while it is read-only.
    pub fn toml_read_only_reason(&self) -> Option<String> {
        self.project
            .as_ref()
            .and_then(|p| p.error())
            .map(|e| format!("sdc.toml is read-only: {e}"))
    }

    /// The textures the bulk panel edits: the selected textures, or the picked chip's members of the selected
    /// sets. A picked kind that has left after a plan is reset to All first, so the chip row and the targets agree.
    /// Empty outside a multi-selection.
    pub fn bulk_targets(&mut self) -> Vec<String> {
        if !self.in_bulk() {
            return vec![];
        }
        let sets = self.selected_sets();
        if sets.len() < 2 {
            return self.selected_items();
        }
        let picked = members_of(self, &sets, self.bulk.kind);
        if picked.is_empty() {
            self.bulk.kind = None;
            return members_of(self, &sets, None);
        }
        picked
    }

    fn bulk_cache_inputs(&self) -> CacheInputs {
        (
            self.multi.clone(),
            self.selection.clone(),
            self.bulk.kind,
            self.items_rev,
            self.inputs_rev,
            self.sets_doc.as_ref().map_or(0, |d| d.rev()),
            format!("{:?}", self.target),
        )
    }

    /// `bulk_targets` and `bulk_fields`, recomputed only when the selection, the kind, the target or the item or
    /// input revisions change.
    pub fn bulk_view(&mut self) -> std::sync::Arc<BulkView> {
        if let Some(c) = &self.bulk.cache {
            if c.inputs == self.bulk_cache_inputs() {
                return c.view.clone();
            }
        }
        let keys = self.bulk_targets();
        let rows = bulk_fields(self, &keys);
        let view = std::sync::Arc::new(BulkView { keys, rows });
        // Taken after `bulk_targets`, which can reset a vanished kind.
        let computes = self.bulk.cache.as_ref().map_or(0, |c| c.computes) + 1;
        self.bulk.cache = Some(BulkCache {
            inputs: self.bulk_cache_inputs(),
            view: view.clone(),
            computes,
        });
        view
    }

    /// How many times `bulk_view` has recomputed since the selection last changed (tests).
    pub fn bulk_computes(&self) -> usize {
        self.bulk.cache.as_ref().map_or(0, |c| c.computes)
    }

    /// F3a D4 "Select members of kind": the selected sets' members of `kind` become a texture selection, the first
    /// one active.
    pub fn select_members_of_kind(&mut self, kind: Option<SlotKind>) {
        let keys = members_of(self, &self.selected_sets(), kind);
        let Some(first) = keys.first().cloned() else {
            return;
        };
        self.set_selection(keys, Some(first), false);
    }

    /// F3a D4: a right-click on an unselected texture selects it alone; on a selected one it keeps the selection.
    pub fn right_click_item(&mut self, key: &str) {
        if !self.is_selected(key, false) {
            self.select_item(key);
        }
    }

    pub fn right_click_set(&mut self, key: &str) {
        if !self.is_selected(key, true) {
            self.select_set(key);
        }
    }

    /// A menu's title: "2 textures selected", "3 sets selected".
    pub fn menu_title(&self, sets: bool) -> String {
        let n = self.selected(sets).len();
        let noun = match (sets, n) {
            (true, 1) => "set",
            (true, _) => "sets",
            (false, 1) => "texture",
            (false, _) => "textures",
        };
        format!("{n} {noun} selected")
    }

    /// F3a D4: a tile's menu.
    pub fn item_menu(&self) -> Vec<MenuEntry> {
        let n = self.selected_items().len();
        let locked = self.toml_read_only_reason();
        let active = match &self.selection {
            Selection::Item(k) => Some(k.as_str()),
            _ => None,
        };
        let name = active
            .and_then(|k| self.item(k))
            .map_or(String::new(), |i| i.name.clone());
        let has_fields = active.is_some_and(|k| {
            self.project.as_ref().is_some_and(|p| {
                all_fields()
                    .iter()
                    .any(|(f, _)| p.file_value(k, f).is_some())
            })
        });
        let copy_off = (n < 2)
            .then(|| "select two or more textures".to_string())
            .or_else(|| locked.clone())
            .or_else(|| (!has_fields).then(|| format!("{name} has no settings of its own")));
        vec![
            MenuEntry {
                action: MenuAction::EditSettings,
                label: format!("Edit settings for {n} selected"),
                disabled: None,
            },
            MenuEntry {
                action: MenuAction::CopyFromActive,
                label: format!(
                    "Copy settings from {name} to {} selected…",
                    n.saturating_sub(1)
                ),
                disabled: copy_off,
            },
            MenuEntry {
                action: MenuAction::ResetAll,
                label: format!("Reset all settings for {n} selected…"),
                disabled: locked,
            },
        ]
    }

    /// F3a D4: a pill's menu while two or more sets are selected.
    pub fn set_menu(&self) -> Vec<MenuEntry> {
        let sets = self.selected_sets();
        let n = sets.len();
        let mut v = vec![
            MenuEntry {
                action: MenuAction::EditSettings,
                label: format!("Edit settings for {n} selected"),
                disabled: None,
            },
            MenuEntry {
                action: MenuAction::ResetAll,
                label: format!("Reset all settings for {n} selected…"),
                disabled: self.toml_read_only_reason(),
            },
        ];
        v.extend(kind_chips(self, &sets).into_iter().map(|c| MenuEntry {
            action: MenuAction::SelectMembersOfKind(c.kind),
            label: format!("{} ({})", c.label, c.members.len()),
            disabled: None,
        }));
        v
    }

    /// F3a Task 8: the menu of slot card `kind` of set `set` (an id): the Slot commands window's commands, From
    /// another set (each other set's member of that kind that writes an output), then Lock or Unlock. A locked slot
    /// offers only Unlock.
    pub fn slot_menu(&self, set: &str, kind: SlotKind) -> Vec<MenuEntry> {
        let views = super::sets::views(self);
        let Some(v) = views.iter().find(|v| same_id(&v.id, set)) else {
            return vec![];
        };
        let (id, n) = (v.id.clone(), kind.slot_number());
        let locked = v.locks.contains(&n);
        let read_only = self
            .sets_doc
            .as_ref()
            .and_then(|d| d.error())
            .map(|e| format!("sdc-project.toml is read-only: {e}"));
        let off = read_only
            .clone()
            .or_else(|| locked.then(|| SLOT_LOCKED.to_string()));
        let current = v.settings.get(&format!("slot{n}"));
        let cmd = |cmd| MenuAction::Slot {
            set: id.clone(),
            n,
            cmd,
        };
        let links: Vec<MenuEntry> = views
            .iter()
            .filter(|o| !same_id(&o.id, &id))
            .flat_map(|o| o.members.iter().map(move |k| (o, k)))
            .filter_map(|(o, k)| {
                let it = self.item(k).filter(|i| slot_of(i) == Some(kind))?;
                member_game_path(self, k)?;
                Some(MenuEntry {
                    action: MenuAction::SlotLink {
                        set: id.clone(),
                        n,
                        item: k.clone(),
                    },
                    label: format!("{} — {}", o.label, it.name),
                    disabled: None,
                })
            })
            .collect();
        let mut menu = vec![
            MenuEntry {
                action: cmd(SlotCommand::FromSet),
                label: format!("From this set ({})", slot_gives(self, &id, n)),
                disabled: off
                    .clone()
                    .or_else(|| current.is_none().then(|| "already from this set".into())),
            },
            MenuEntry {
                action: MenuAction::SlotPath { set: id.clone(), n },
                label: "Path…".into(),
                disabled: off.clone(),
            },
            MenuEntry {
                action: cmd(SlotCommand::Clear),
                label: "Clear".into(),
                disabled: off.clone().or_else(|| {
                    (current == Some(&Value::from(""))).then(|| "already cleared".into())
                }),
            },
            MenuEntry {
                action: MenuAction::FromAnotherSet,
                label: "From another set".into(),
                disabled: off.or_else(|| {
                    links
                        .is_empty()
                        .then(|| format!("no other set has a {}", kind.label()))
                }),
            },
        ];
        menu.extend(links);
        menu.push(MenuEntry {
            action: MenuAction::SlotLock {
                set: id.clone(),
                n,
                on: !locked,
            },
            label: if locked { "Unlock" } else { "Lock" }.into(),
            disabled: read_only,
        });
        menu
    }

    /// Run a menu entry; a refused slot command comes back as its reason.
    pub fn run_menu(&mut self, a: MenuAction) -> Option<String> {
        match a {
            MenuAction::EditSettings => {
                self.collapse();
                self.bulk.kind = None;
            }
            MenuAction::CopyFromActive => self.open_copy_settings(),
            MenuAction::ResetAll => self.ask_reset_all(),
            MenuAction::SelectMembersOfKind(k) => self.select_members_of_kind(k),
            MenuAction::Slot { set, n, cmd } => return self.set_slot_command(&set, n, cmd).err(),
            MenuAction::SlotPath { .. } | MenuAction::FromAnotherSet => {}
            MenuAction::SlotLink { set, n, item } => {
                let Some(p) = member_game_path(self, &item) else {
                    return Some(format!("{item} writes no output"));
                };
                return self.set_slot_command(&set, n, SlotCommand::Path(p)).err();
            }
            MenuAction::SlotLock { set, n, on } => return self.set_slot_lock(&set, n, on).err(),
        }
        None
    }
}
