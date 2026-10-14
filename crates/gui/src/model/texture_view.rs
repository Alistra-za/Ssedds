//! The Textures column's filters, sort order and tile size (item 8, DECISIONS AG4 and AG46-AG55), and the cached list
//! of what it shows.
use super::items::{ItemStatus, TextureItem};
use super::{counts_toward_badge, AppModel};
use engine::config::Backend;
use std::collections::HashSet;
use std::sync::Arc;

/// Set membership filter: the combo next to the "Not in a set" toggle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SetFilter {
    #[default]
    Any,
    In,
    NotIn,
}

impl SetFilter {
    pub const ALL: [SetFilter; 3] = [SetFilter::Any, SetFilter::In, SetFilter::NotIn];
    /// The `gui.toml` name.
    pub fn name(self) -> &'static str {
        match self {
            SetFilter::Any => "any",
            SetFilter::In => "in",
            SetFilter::NotIn => "not-in",
        }
    }
    pub fn from_name(n: &str) -> Option<SetFilter> {
        Self::ALL.into_iter().find(|s| s.name() == n)
    }
    pub fn label(self) -> &'static str {
        match self {
            SetFilter::Any => "Any",
            SetFilter::In => "In a set",
            SetFilter::NotIn => "Not in a set",
        }
    }
}

/// Every filter of the column; they combine with AND, and with the search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub needs_decision: bool,
    pub warnings: bool,
    pub failed: bool,
    pub not_in_set: bool,
    /// A sub-type id (`Subtype::id`), a format id (`Format::id`) or a backend id; `None` is Any.
    pub subtype: Option<String>,
    pub format: Option<String>,
    pub encoder: Option<String>,
    pub set: SetFilter,
}

/// The sort order (AG4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Status,
}

impl SortKey {
    pub const ALL: [SortKey; 3] = [SortKey::Name, SortKey::Size, SortKey::Status];
    pub fn name(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Status => "status",
        }
    }
    pub fn from_name(n: &str) -> Option<SortKey> {
        Self::ALL.into_iter().find(|s| s.name() == n)
    }
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Size => "Source size",
            SortKey::Status => "Status",
        }
    }
}

/// What the column remembers besides its filters: the sort and the tile size (points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnPrefs {
    pub sort: SortKey,
    pub tile: f32,
}

impl ColumnPrefs {
    pub const TILE_RANGE: (f32, f32) = (64.0, 192.0);
    pub const TILE_DEFAULT: f32 = 96.0;
}

impl Default for ColumnPrefs {
    fn default() -> Self {
        ColumnPrefs {
            sort: SortKey::Name,
            tile: Self::TILE_DEFAULT,
        }
    }
}

/// The encoders the filter can name, in the order it lists them, by `Backend` id.
const ENCODERS: [(&str, &str); 5] = [
    ("bc7e", "bc7e"),
    ("rgbcx", "rgbcx"),
    ("ispc_bc6h", "ispc (BC6H)"),
    ("nvtt", "NVTT"),
    ("uncompressed", "uncompressed"),
];

/// The label of an encoder id.
pub fn encoder_label(id: &str) -> &str {
    ENCODERS.iter().find(|e| e.0 == id).map_or(id, |e| e.1)
}

fn backend_id(b: Backend) -> String {
    serde_json::to_value(b)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The values the combos list: only those in the folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterOptions {
    pub subtypes: Vec<String>,
    pub formats: Vec<String>,
    pub encoders: Vec<String>,
}

/// The cached option lists and index list, by what they were built from.
#[derive(Default)]
pub(crate) struct TextureViewCache {
    options: Option<(u64, Arc<FilterOptions>)>,
    list: Option<(ListKey, Arc<Vec<usize>>)>,
    builds: u64,
}

#[derive(PartialEq)]
struct ListKey {
    items: u64,
    filter: Filter,
    search: String,
    sort: SortKey,
}

/// Lower is shown first: errors, warnings, undecided, ok (AG4).
fn status_rank(i: &TextureItem) -> u8 {
    if matches!(i.status, ItemStatus::Failed(_) | ItemStatus::Error(_)) {
        0
    } else if i.warnings.iter().any(counts_toward_badge) {
        1
    } else if i.status == ItemStatus::NeedsDecision {
        2
    } else {
        3
    }
}

/// The source's size in bytes: the job's stamp, or a skipped DDS's own; 0 when neither was taken.
fn source_size(i: &TextureItem) -> u64 {
    i.job
        .as_ref()
        .and_then(|j| j.source)
        .or(i.stamp)
        .map_or(0, |s| s.size)
}

impl AppModel {
    /// The sets of more than one member, by key (the rule of "in a set").
    fn multi_sets(&self) -> HashSet<&str> {
        self.sets
            .iter()
            .filter(|s| s.members.len() > 1)
            .map(|s| s.key.as_str())
            .collect()
    }

    /// The sub-types, formats and encoders of the folder's textures, for the combos. Cached by the items revision.
    pub fn filter_options(&self) -> Arc<FilterOptions> {
        let mut c = self.texture_cache.borrow_mut();
        if let Some((rev, o)) = &c.options {
            if *rev == self.items_rev() {
                return o.clone();
            }
        }
        let mut subtypes = std::collections::BTreeSet::new();
        let mut formats = std::collections::BTreeSet::new();
        let mut backends = HashSet::new();
        for p in self
            .items
            .iter()
            .filter_map(|i| i.job.as_ref())
            .map(|j| &j.profile)
        {
            subtypes.insert(p.subtype.id());
            formats.insert(p.format.id());
            backends.insert(backend_id(p.encoder.backend));
        }
        let o = Arc::new(FilterOptions {
            subtypes: subtypes.into_iter().collect(),
            formats: formats.into_iter().collect(),
            encoders: ENCODERS
                .iter()
                .filter(|e| backends.contains(e.0))
                .map(|e| e.0.to_string())
                .collect(),
        });
        c.options = Some((self.items_rev(), o.clone()));
        o
    }

    /// How many times the index list was built (tests: once per change, not per frame).
    pub fn visible_builds(&self) -> u64 {
        self.texture_cache.borrow().builds
    }

    /// The indices into `items` the column shows, sorted. Built again only when the filters, the search, the sort or
    /// the items changed.
    pub fn visible_shared(&self) -> Arc<Vec<usize>> {
        let key = ListKey {
            items: self.items_rev(),
            filter: self.filter.clone(),
            search: self.search.clone(),
            sort: self.prefs.sort,
        };
        let mut c = self.texture_cache.borrow_mut();
        if let Some((k, l)) = &c.list {
            if *k == key {
                return l.clone();
            }
        }
        let list = Arc::new(self.build_visible());
        c.builds += 1;
        c.list = Some((key, list.clone()));
        list
    }

    fn build_visible(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        let f = &self.filter;
        let multi = self.multi_sets();
        let in_set = |i: &TextureItem| i.set_key.as_deref().is_some_and(|k| multi.contains(k));
        let mut out: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                let profile = i.job.as_ref().map(|j| &j.profile);
                (q.is_empty() || i.key.to_lowercase().contains(&q))
                    && (!f.needs_decision || i.status == ItemStatus::NeedsDecision)
                    && (!f.warnings || i.warnings.iter().any(counts_toward_badge))
                    && (!f.failed
                        || matches!(i.status, ItemStatus::Failed(_) | ItemStatus::Error(_)))
                    && (!f.not_in_set || !in_set(i))
                    && match f.set {
                        SetFilter::Any => true,
                        SetFilter::In => in_set(i),
                        SetFilter::NotIn => !in_set(i),
                    }
                    && f.subtype
                        .as_ref()
                        .is_none_or(|s| profile.is_some_and(|p| p.subtype.id() == *s))
                    && f.format
                        .as_ref()
                        .is_none_or(|s| profile.is_some_and(|p| p.format.id() == *s))
                    && f.encoder.as_ref().is_none_or(|s| {
                        profile.is_some_and(|p| backend_id(p.encoder.backend) == *s)
                    })
            })
            .map(|(n, _)| n)
            .collect();
        // `items` is already in name order (the key's, folder then file), and the sort is stable: that is the tiebreak.
        match self.prefs.sort {
            SortKey::Name => {}
            SortKey::Size => out.sort_by_key(|&n| std::cmp::Reverse(source_size(&self.items[n]))),
            SortKey::Status => out.sort_by_key(|&n| status_rank(&self.items[n])),
        }
        out
    }

    pub fn visible_items(&self) -> Vec<usize> {
        self.visible_shared().to_vec()
    }

    /// How many textures the filters and the search hide.
    pub fn hidden_by_filters(&self) -> usize {
        self.items.len() - self.visible_shared().len()
    }

    /// Any toggle, combo or search text is on.
    pub fn filters_active(&self) -> bool {
        self.filter != Filter::default() || !self.search.is_empty()
    }

    /// "Clear filters": the toggles, the combos and the search; not the sort or the tile size.
    pub fn clear_filters(&mut self) {
        self.filter = Filter::default();
        self.search.clear();
    }

    /// How many textures the "Not in a set" filter would show.
    pub fn not_in_set_count(&self) -> usize {
        let multi = self.multi_sets();
        self.items
            .iter()
            .filter(|i| !i.set_key.as_deref().is_some_and(|k| multi.contains(k)))
            .count()
    }
}
