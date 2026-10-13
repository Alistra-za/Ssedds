//! M6 Track F3a (design `2026-10-13-sdc-m6-f3a-design.md`): the multi-selection, the bulk edit plan and its
//! commands. No egui types.
use super::fields::{
    all_fields, field_hidden, field_view, filter_option_lock, views_of, FieldView, EDGE_FIELD,
    FIELDS, FILTER_FIELD, LEVELS_FIELD,
};
use super::sets::SlotKind;
use super::{AppModel, Selection};
use engine::config::MipFilter;
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
fn skip_of(
    job: Option<&ResolvedJob>,
    view: Option<&FieldView>,
    field: &str,
    op: &BulkOp,
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
        return Some(lock.into());
    }
    if let (BulkOp::Set(v), "mips.filter") = (op, field) {
        let barred = serde_json::from_value::<MipFilter>(v.clone())
            .ok()
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
            let change = skip_of(job, view.as_ref(), field, op).map_or(Change::Write, Change::Skip);
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

/// The bulk panel's rows over `keys` (F3a D2). Counts are the value-independent ones (`BulkOp::Clear`); the filter
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
            match skip_of(job, view, row.field, &BulkOp::Clear) {
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
