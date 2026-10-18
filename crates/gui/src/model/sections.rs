//! GUI tweaks G4/G6: delete sections, the PGPatcher entries that belong to no set (`[[pgp_sections]]` in
//! `sdc-project.toml`). A section matches shapes by texture (`match1`–`match8` exact, `path_contains`) and may be
//! narrowed to meshes (`nif_filter`). Every section is written with `delete: true`: PGPatcher deletes the trishapes
//! it matches (a vanilla glow shape, say, when the PBR base shape is emissive instead). It is written after every
//! set's entry, while it has a texture match.
use super::sets::{views, SetView};
use super::AppModel;
use crate::pgp::settings::Settings;
use serde_json::{json, Value};
use std::sync::Arc;

/// The keys a section can hold, in the order the JSON writes them.
pub const SECTION_KEYS: [&str; 11] = [
    "match1",
    "match2",
    "match3",
    "match4",
    "match5",
    "match6",
    "match7",
    "match8",
    "path_contains",
    "nif_filter",
    "delete",
];

/// A texture match of a section or an entry, as it is matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TexMatch {
    /// `matchN`: the texture in slot N (1-based) is exactly this path.
    Exact(u8, String),
    /// `path_contains`: the diffuse path contains this text.
    Contains(String),
}

fn text<'a>(s: &'a Settings, key: &str) -> Option<&'a str> {
    s.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// The texture matches of section `s`.
pub fn tex_matches(s: &Settings) -> Vec<TexMatch> {
    let mut out: Vec<TexMatch> = (1..=8u8)
        .filter_map(|n| text(s, &format!("match{n}")).map(|v| TexMatch::Exact(n, v.to_string())))
        .collect();
    if let Some(p) = text(s, "path_contains") {
        out.push(TexMatch::Contains(p.to_string()));
    }
    out
}

/// Section `s` is written to the JSON: it matches a texture.
pub fn written(s: &Settings) -> bool {
    !tex_matches(s).is_empty()
}

/// Why section `s` is not written, when it is not.
pub fn not_written_reason(s: &Settings) -> Option<&'static str> {
    if tex_matches(s).is_empty() {
        Some("this section matches no texture and is not written (a mesh filter alone matches nothing)")
    } else {
        None
    }
}

/// A texture path as an exact match compares it: lower case, `\`, no leading `textures\`, no extension (a source's
/// game path ends in its own image extension; a match path in `.dds`).
fn norm(p: &str) -> String {
    let p = p.trim().replace('/', "\\").to_lowercase();
    let p = p.strip_prefix("textures\\").unwrap_or(&p);
    let (dir, name) = p.rsplit_once('\\').unwrap_or(("", p));
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    if dir.is_empty() {
        stem.to_string()
    } else {
        format!("{dir}\\{stem}")
    }
}

/// A set whose textures a delete section matches: the section (0-based), the set id and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteHit {
    pub section: usize,
    pub set_id: String,
    pub why: String,
}

/// The game paths of set `v`'s textures, by slot number (1-based).
fn slot_paths(m: &AppModel, v: &SetView) -> Vec<(u8, String)> {
    v.slots
        .iter()
        .filter_map(|(kind, fill)| {
            let item = match fill {
                super::sets::SlotFill::Own { item }
                | super::sets::SlotFill::Borrowed { item, .. } => item,
                super::sets::SlotFill::Empty => return None,
            };
            let j = m.item(item)?.job.as_ref()?;
            Some((kind.slot_number(), super::sets::written_game_path(j)))
        })
        .collect()
}

/// G6: every set that a written delete section would delete shapes of: a section's exact match is the set's texture
/// in that slot, or its `path_contains` is in the set's diffuse path. A `nif_filter` narrows it to some meshes,
/// which the hit says. Cached with the set views (REPORT #11).
pub fn delete_hits(m: &AppModel) -> Arc<Vec<DeleteHit>> {
    let key = m.view_key();
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some(h) = &c.hits {
                return h.clone();
            }
        }
    }
    // No borrow is held here: `build_hits` reads the cached views.
    let h = Arc::new(build_hits(m));
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = super::ViewCache {
            key: Some(key),
            ..Default::default()
        };
    }
    c.hits = Some(h.clone());
    h
}

fn build_hits(m: &AppModel) -> Vec<DeleteHit> {
    let sections = m.sections();
    if sections.is_empty() {
        return vec![];
    }
    let views = views(m);
    let mut out = vec![];
    for (i, s) in sections.iter().enumerate().filter(|(_, s)| written(s)) {
        let meshes = text(s, "nif_filter")
            .map(|f| format!(" on meshes whose path contains {f}"))
            .unwrap_or_default();
        for v in views.iter() {
            let paths = slot_paths(m, v);
            let hit = tex_matches(s).into_iter().find_map(|t| match &t {
                TexMatch::Exact(n, p) => {
                    let want = norm(p);
                    paths
                        .iter()
                        .find(|(slot, gp)| slot == n && norm(gp) == want)
                        .map(|(_, gp)| format!("match{n} is its texture {gp}"))
                }
                TexMatch::Contains(c) => {
                    let c = c.replace('/', "\\").to_lowercase();
                    paths
                        .iter()
                        .find(|(slot, gp)| {
                            *slot == 1 && gp.replace('/', "\\").to_lowercase().contains(&c)
                        })
                        .map(|(_, gp)| format!("its diffuse {gp} contains {c}"))
                }
            });
            if let Some(why) = hit {
                out.push(DeleteHit {
                    section: i,
                    set_id: v.id.clone(),
                    why: format!("{why}{meshes}"),
                });
            }
        }
    }
    out
}

impl AppModel {
    /// The delete sections, in file order.
    pub fn sections(&self) -> Vec<Settings> {
        self.sets_doc
            .as_ref()
            .map(|d| d.sections())
            .unwrap_or_default()
    }

    /// Add a delete section matching `path` exactly in slot 1 (empty: a section with no match yet). One undo step.
    pub fn add_section(&mut self, path: &str) {
        if !self.sets_editable() {
            return;
        }
        let mut keys = Settings::new();
        keys.insert("delete".into(), json!(true));
        if !path.trim().is_empty() {
            keys.insert("match1".into(), json!(path.trim()));
        }
        self.edit_docs("Add delete section", None, false, |_, d| {
            d.add_section(&keys)
        });
    }

    pub fn remove_section(&mut self, i: usize) {
        if !self.sets_editable() {
            return;
        }
        self.edit_docs("Remove delete section", None, false, |_, d| {
            d.remove_section(i)
        });
    }

    /// Several keys of section `i` as one undo step (a match pill's mode switch, trace-debug #56).
    pub fn set_section_values(&mut self, i: usize, edits: Vec<(String, Option<Value>)>) {
        self.batch_edit("Change match", |m| {
            for (k, v) in edits {
                m.set_section_value(i, &k, v);
            }
        });
    }

    /// Set (or with `None` remove) key `key` of section `i`. Edits of one key merge into one undo step.
    pub fn set_section_value(&mut self, i: usize, key: &str, v: Option<Value>) {
        if !self.sets_editable() || !SECTION_KEYS.contains(&key) {
            return;
        }
        let merge = Some(format!("section{i}:{key}"));
        let k = key.to_string();
        self.edit_docs(&format!("Change {key}"), merge, false, move |_, d| {
            d.set_section_value(i, &k, v)
        });
    }
}
