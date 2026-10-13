//! M6 Track F3a (design `2026-10-13-sdc-m6-f3a-design.md`): the multi-selection, the bulk edit plan and its
//! commands. No egui types.
use super::sets::SlotKind;
use super::{AppModel, Selection};

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
