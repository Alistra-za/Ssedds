//! The Sets column's search box, filter pills and shader filter (layout v2, owner addition R17). Filters only hide:
//! nothing here touches the selection, the multi-selection or a document (Review Focus 5). Ctrl+A and Shift-click
//! ranges on sets follow the visible sets, as they do on textures (K18).
use super::sets::{self, SetView};
use super::{bottom, cm_json, AppModel, ViewKey};
use engine::shader::Shader;
use std::sync::Arc;

/// Every part of the filter; they combine with AND, as the Textures filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetsFilter {
    pub search: String,
    pub attention: bool,
    pub ready: bool,
    pub no_json: bool,
    pub shader: Option<Shader>,
}

/// What each pill shows, over the sets with a key (K20: a created set with no key counts toward none).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetCounts {
    pub attention: usize,
    pub ready: usize,
    pub no_json: usize,
}

/// The cached list and counts, by what they were built from (K17: Ready reads `problems()`, which can change while
/// the views key does not).
#[derive(Default)]
pub(crate) struct SetsFilterCache {
    list: Option<(ListKey, Arc<Vec<usize>>)>,
    counts: Option<(CountsKey, SetCounts)>,
}

#[derive(Clone)]
struct CountsKey {
    views: ViewKey,
    problems: Arc<Vec<bottom::Problem>>,
}

impl PartialEq for CountsKey {
    fn eq(&self, o: &Self) -> bool {
        self.views == o.views && Arc::ptr_eq(&self.problems, &o.problems)
    }
}

#[derive(PartialEq)]
struct ListKey {
    of: CountsKey,
    filter: SetsFilter,
}

/// Whether the set writes a JSON: a complex-material export (A8), or a PBR Landscape set's CS records or PGPatcher
/// entry, or a PGPatcher JSON set; never an Advanced skin set.
pub fn writes_json(m: &AppModel, v: &SetView) -> bool {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    writes_json_in(m, &entries, v)
}

fn writes_json_in(m: &AppModel, entries: &[crate::sets_doc::SetEntry], v: &SetView) -> bool {
    cm_json::exports(entries, &v.id)
        || if sets::is_pbr_landscape(v) {
            m.cs_records_written(v) || m.pgp_written(v)
        } else if v.skin {
            false
        } else {
            sets::is_json_set(v, m.target)
        }
}

impl AppModel {
    fn sets_counts_key(&self) -> CountsKey {
        CountsKey {
            views: self.view_key(),
            problems: bottom::problems(self),
        }
    }

    /// The indices into `sets::views` the column shows, in display order. Built again only when the filter, the
    /// views or the problems changed.
    pub fn visible_sets(&self) -> Arc<Vec<usize>> {
        // Every input first: `problems` and `set_ready` borrow the view cache, never while this cache is borrowed.
        let views = sets::views(self);
        let key = ListKey {
            of: self.sets_counts_key(),
            filter: self.sets_filter.clone(),
        };
        if let Some((k, l)) = &self.sets_filter_cache.borrow().list {
            if *k == key {
                return l.clone();
            }
        }
        let flagged = bottom::flagged(self);
        let f = &self.sets_filter;
        let q = engine::sets::fold_id(f.search.trim());
        let entries = self
            .sets_doc
            .as_ref()
            .map(|d| d.entries())
            .unwrap_or_default();
        let list: Arc<Vec<usize>> = Arc::new(if !self.sets_filter_active() {
            (0..views.len()).collect()
        } else {
            views
                .iter()
                .enumerate()
                .filter(|(_, v)| {
                    // A created set with no key passes only an empty filter.
                    !v.key.is_empty()
                        && (q.is_empty() || engine::sets::fold_id(&v.id).contains(&q))
                        && (!f.attention || !bottom::set_ready_in(&flagged, v))
                        && (!f.ready || bottom::set_ready_in(&flagged, v))
                        && (!f.no_json || !writes_json_in(self, &entries, v))
                        && f.shader
                            .is_none_or(|s| v.shader.as_ref().map(|p| p.shader) == Some(s))
                })
                .map(|(n, _)| n)
                .collect()
        });
        self.sets_filter_cache.borrow_mut().list = Some((key, list.clone()));
        list
    }

    /// The pill counts over the sets with a key, cached as the list is.
    pub fn set_counts(&self) -> SetCounts {
        let views = sets::views(self);
        let key = self.sets_counts_key();
        if let Some((k, c)) = &self.sets_filter_cache.borrow().counts {
            if *k == key {
                return *c;
            }
        }
        let entries = self
            .sets_doc
            .as_ref()
            .map(|d| d.entries())
            .unwrap_or_default();
        let flagged = bottom::flagged(self);
        let mut c = SetCounts::default();
        for v in views.iter().filter(|v| !v.key.is_empty()) {
            let ready = bottom::set_ready_in(&flagged, v);
            c.ready += usize::from(ready);
            c.attention += usize::from(!ready);
            c.no_json += usize::from(!writes_json_in(self, &entries, v));
        }
        self.sets_filter_cache.borrow_mut().counts = Some((key, c));
        c
    }

    /// Any pill, the shader or search text is on.
    pub fn sets_filter_active(&self) -> bool {
        let f = &self.sets_filter;
        f.attention || f.ready || f.no_json || f.shader.is_some() || !f.search.trim().is_empty()
    }

    /// "Clear filter": the pills, the shader and the search.
    pub fn clear_sets_filter(&mut self) {
        self.sets_filter = SetsFilter::default();
    }
}

/// Task 5: how many set cards have been laid out, for the virtualisation pin.
#[doc(hidden)]
pub fn set_cards_drawn(m: &AppModel) -> u64 {
    m.set_cards_drawn.get()
}
