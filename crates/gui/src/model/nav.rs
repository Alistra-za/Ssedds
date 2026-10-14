//! Keyboard navigation (item 5) and the command palette's search (item 19). The keys themselves are in
//! `ui::actions`; this is what they do to the model.
use super::preview::SourceState;
use super::view::Channel;
use super::{AppModel, Selection, ViewKey};
use std::sync::Arc;

/// The palette lists at most this many results.
pub const PALETTE_CAP: usize = 200;

/// What the keyboard and the palette keep between frames.
#[derive(Default)]
pub struct Nav {
    /// Tiles per row in the Textures grid as last drawn: Up and Down move by this many.
    pub cols: usize,
    /// The texture the arrow keys last moved to. Shift+arrow keeps the active texture as the anchor (F3a D1), so the
    /// moving end of the range is kept here.
    pub cursor: Option<String>,
    /// A texture or set to scroll into view in its column on the next frame.
    pub reveal_item: Option<String>,
    pub reveal_set: Option<String>,
    /// The F1 shortcut sheet is open.
    pub sheet: bool,
    /// The Ctrl+K palette, while open.
    pub palette: Option<Palette>,
    /// The textures and sets that match a query, by query and by what the plan and documents were.
    cache: Option<(String, ViewKey, Arc<Vec<Hit>>)>,
}

/// The palette's search field and the highlighted row.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub sel: usize,
    /// The search field takes the focus on the frame the palette opens.
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// A `ui::actions` id.
    Action(&'static str),
    Texture(String),
    Set(String),
}

/// One palette result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 0 prefix, 1 substring, 2 subsequence.
    pub rank: u8,
    /// 0 action, 1 texture, 2 set.
    pub group: u8,
    pub label: String,
    /// Shown dimmed beside the label (a texture's path, an action's keys).
    pub detail: String,
    pub pick: Pick,
}

/// How `name` matches `query`, case-insensitively: 0 prefix, 1 substring, 2 subsequence ("fuzzy-lite"), `None` no
/// match. An empty query matches everything as a prefix.
pub fn rank(query: &str, name: &str) -> Option<u8> {
    let (q, n) = (query.to_lowercase(), name.to_lowercase());
    if n.starts_with(&q) {
        return Some(0);
    }
    if n.contains(&q) {
        return Some(1);
    }
    let mut rest = n.chars();
    q.chars().all(|c| rest.any(|m| m == c)).then_some(2)
}

fn sort_cap(hits: &mut Vec<Hit>) {
    hits.sort_by_cached_key(|h| (h.rank, h.group, h.label.to_lowercase()));
    hits.truncate(PALETTE_CAP);
}

impl AppModel {
    /// The palette's results for `query`: `actions` (id, label, keys), then textures by name, then sets by name, each
    /// ranked prefix, substring, subsequence; at most [`PALETTE_CAP`]. The textures and sets are searched again only
    /// when the query or the plan changed, not every frame.
    pub fn palette_hits(
        &mut self,
        query: &str,
        actions: &[(&'static str, &str, String)],
    ) -> Vec<Hit> {
        let key = self.view_key();
        let stale = !matches!(&self.nav.cache, Some((q, k, _)) if q == query && *k == key);
        if stale {
            let mut found: Vec<Hit> = self
                .items
                .iter()
                .filter_map(|i| {
                    Some(Hit {
                        rank: rank(query, &i.name)?,
                        group: 1,
                        label: i.name.clone(),
                        detail: i.key.clone(),
                        pick: Pick::Texture(i.key.clone()),
                    })
                })
                .collect();
            found.extend(
                super::sets::views(self)
                    .iter()
                    .filter(|v| !v.key.is_empty())
                    .filter_map(|v| {
                        Some(Hit {
                            rank: rank(query, &v.label)?,
                            group: 2,
                            label: v.label.clone(),
                            detail: "set".to_string(),
                            pick: Pick::Set(v.key.clone()),
                        })
                    }),
            );
            sort_cap(&mut found);
            self.nav.cache = Some((query.to_string(), key, Arc::new(found)));
        }
        let mut hits: Vec<Hit> = actions
            .iter()
            .filter_map(|(id, label, keys)| {
                Some(Hit {
                    rank: rank(query, label)?,
                    group: 0,
                    label: label.to_string(),
                    detail: keys.clone(),
                    pick: Pick::Action(id),
                })
            })
            .collect();
        if let Some((_, _, found)) = &self.nav.cache {
            hits.extend(found.iter().cloned());
        }
        sort_cap(&mut hits);
        hits
    }

    /// A texture or set picked in the palette: the same as clicking it, and its column scrolls to it.
    pub fn palette_pick(&mut self, pick: &Pick) {
        match pick {
            Pick::Action(_) => {}
            Pick::Texture(k) => {
                self.click_item(k, false, false);
                self.nav.cursor = Some(k.clone());
                self.nav.reveal_item = Some(k.clone());
            }
            Pick::Set(k) => {
                self.click_set(k, false, false);
                self.nav.reveal_set = Some(k.clone());
            }
        }
    }

    /// Where the arrow keys start in `order`: the texture they last moved to while it is still selected, else the
    /// active texture.
    fn grid_from(&self, order: &[String]) -> Option<usize> {
        let cursor = self
            .nav
            .cursor
            .as_deref()
            .filter(|c| self.is_selected(c, false));
        let active = match &self.selection {
            Selection::Item(k) => Some(k.as_str()),
            _ => None,
        };
        cursor
            .into_iter()
            .chain(active)
            .find_map(|k| order.iter().position(|o| o == k))
    }

    /// Item 5: an arrow key in the Textures grid. `step` is ±1 (Left, Right) or ± the tiles per row (Up, Down), in the
    /// displayed order (search and filters applied); a move off the grid does nothing. With nothing to start from,
    /// the first texture. `extend` (Shift) selects the range from the active texture, as a Shift-click does (F3a D1).
    pub fn grid_move(&mut self, step: isize, extend: bool) {
        let order = self.visible_keys();
        if order.is_empty() {
            return;
        }
        let to = match self.grid_from(&order) {
            None => 0,
            Some(i) => match i.checked_add_signed(step).filter(|t| *t < order.len()) {
                Some(t) => t,
                None => return,
            },
        };
        let key = order[to].clone();
        self.click_item(&key, false, extend);
        self.nav.cursor = Some(key.clone());
        self.nav.reveal_item = Some(key);
    }

    /// Up and Down move by a row of the grid.
    pub fn grid_row_step(&self) -> isize {
        self.nav.cols.max(1) as isize
    }

    /// Enter or Space in the grid: the expanded preview of the active texture.
    pub fn open_active(&mut self) {
        if let Selection::Item(k) = self.selection.clone() {
            self.expand(&k);
        }
    }

    /// `[` and `]` in the expanded preview: the previous or next texture in the displayed order.
    pub fn expand_step(&mut self, step: isize) {
        let Some(cur) = self.expanded.clone() else {
            return;
        };
        let order = self.visible_keys();
        let Some(to) = order
            .iter()
            .position(|k| *k == cur)
            .and_then(|i| i.checked_add_signed(step))
            .filter(|t| *t < order.len())
        else {
            return;
        };
        let key = order[to].clone();
        self.expand(&key);
        self.nav.cursor = Some(key.clone());
        self.nav.reveal_item = Some(key);
    }

    /// 1-6 in the expanded preview: the channel buttons in the order they are drawn (their shader ids run 0-5 in
    /// that order).
    pub fn pick_channel(&mut self, n: u32) {
        if let Some(c) = Channel::from_id(n) {
            self.view.channel = c;
        }
    }

    /// The mips of the expanded texture the preview can show: their sizes, the first one previewed and one past the
    /// last. From the encoded result when there is one, else the source; empty before either.
    pub fn preview_mips(&self) -> (Vec<(u32, u32)>, usize, usize) {
        if let Some(o) = self.preview.encoded.shown() {
            let s = &o.result.source;
            return (
                s.dims.clone(),
                s.first_level,
                s.first_level + o.result.levels.len(),
            );
        }
        match &self.preview.source {
            SourceState::Ready(s) if !s.levels.is_empty() => (
                s.dims.clone(),
                s.first_level,
                s.first_level + s.levels.len(),
            ),
            _ => (vec![], 0, 0),
        }
    }

    /// PageDown (+1, a smaller mip) and PageUp (-1) in the expanded preview, within the previewed mips. The zoom
    /// follows as the mip slider's does.
    pub fn step_mip(&mut self, step: isize) {
        let (dims, first, end) = self.preview_mips();
        if end <= first || end > dims.len() {
            return;
        }
        let cur = self.view.mip.clamp(first, end - 1);
        let to = (cur as isize + step).clamp(first as isize, end as isize - 1) as usize;
        if to != cur {
            self.view.set_mip(to, dims[cur], dims[to]);
        }
    }
}
