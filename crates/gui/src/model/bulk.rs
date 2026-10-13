//! M6 Track F3a (design `2026-10-13-sdc-m6-f3a-design.md`): the multi-selection, the bulk edit plan and its
//! commands. No egui types.
use super::fields::{
    all_fields, field_hidden, field_view, filter_option_lock, views_of, FieldView, EDGE_FIELD,
    FIELDS, FILTER_FIELD, LEVELS_FIELD,
};
use super::sets::SlotKind;
use super::{AppModel, Dialog, Selection};
use engine::config::{MipFilter, Subtype};
use engine::policy::ResolvedJob;
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
/// blocked by a field lock (R6): dropping a stale `[files]` entry is harmless and the only way to remove it.
fn skip_of(
    job: Option<&ResolvedJob>,
    view: Option<&FieldView>,
    field: &str,
    op: Option<&BulkOp>,
) -> Option<String> {
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
    keys.iter()
        .map(|k| {
            let job = m.item(k).and_then(|i| i.job.as_ref());
            let view = job.and_then(|j| field_view(m.target, j, field));
            let change =
                skip_of(job, view.as_ref(), field, Some(op)).map_or(Change::Write, Change::Skip);
            (k.clone(), change)
        })
        .collect()
}

/// One row of the bulk panel (F3a D2).
#[derive(Debug, Clone, PartialEq)]
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
/// lock of a picked value shows in the report.
/// shortcut: rebuilt every frame, one profile serialisation per texture; cache under `items_rev` if a selection of
/// thousands shows in a profile.
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
    for k in keys {
        let job = m.item(k).and_then(|i| i.job.as_ref());
        // One profile serialisation per texture, not one per row.
        let views = job.map_or_else(Vec::new, |j| views_of(&fields, m.target, j));
        for (n, row) in rows.iter_mut().enumerate() {
            let view = views.get(n);
            match skip_of(job, view, row.field, None) {
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
        self.batch_edit(&format!("{verb} {label} on {written} textures"), |m| {
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
        self.bulk.report = Some(report(label, &plan));
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
        self.batch_edit(&format!("Reset all settings on {n} textures"), |m| {
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
        self.bulk.report = Some(format!("Reset all settings on {n} textures"));
    }
}
