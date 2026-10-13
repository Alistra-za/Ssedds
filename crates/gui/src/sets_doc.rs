//! `sdc-project.toml` (M3b spec §7.1): the user's sets and JSON settings, next to `sdc.toml`. Format-preserving
//! like `ProjectDoc`; written only on Save or Export. A file that does not parse makes set and JSON editing
//! read-only (DECISIONS P8); `sdc.toml` editing is unaffected.
use crate::pgp::settings::Settings;
use crate::project::{dotted_table, json_to_toml, set_value, sub_table, toml_to_json, SaveError};
use engine::policy::SourceStamp;
use engine::sets::{fold_id, same_id};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, TableLike, Value};

pub const FILE_NAME: &str = "sdc-project.toml";
/// Keys of a `[[sets]]` table that are not JSON settings.
const RESERVED: [&str; 7] = [
    "id",
    "created",
    "renamed_from",
    "add",
    "remove",
    "borrow",
    "shader",
];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetEntry {
    pub id: String,
    pub created: bool,
    pub renamed_from: Option<String>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    /// Slot number (1–8) → the id of the set whose finished output this set uses.
    pub borrow: BTreeMap<u8, String>,
    /// M6: the explicit shader choice (`shader = "<id>"`); an unknown id reads as `None` and stays in the file.
    pub shader: Option<engine::shader::Shader>,
    pub settings: Settings,
}

/// Source of `SetsDoc::rev`: one counter for every document, so a replaced document never reuses a revision (DECISIONS Note Y7).
static NEXT_REV: AtomicU64 = AtomicU64::new(1);
fn next_rev() -> u64 {
    NEXT_REV.fetch_add(1, Ordering::Relaxed)
}

pub struct SetsDoc {
    path: PathBuf,
    doc: DocumentMut,
    disk: Option<SourceStamp>,
    dirty: bool,
    error: Option<String>,
    rev: u64,
}

/// Flatten a table into `prefix.key` paths; values only (nested tables recurse).
fn flatten(t: &dyn TableLike, prefix: &str, out: &mut Settings) {
    for (k, item) in t.iter() {
        let path = if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        };
        match item.as_table_like() {
            Some(sub) if !item.is_value() || item.as_inline_table().is_some() => {
                flatten(sub, &path, out)
            }
            _ => {
                if let Some(v) = toml_to_json(item) {
                    out.insert(path, v);
                }
            }
        }
    }
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn entry_of(t: &Table) -> Option<SetEntry> {
    let id = t.get("id")?.as_str()?.to_string();
    let mut settings = Settings::new();
    for (k, item) in t.iter() {
        if RESERVED.contains(&k) {
            continue;
        }
        match item.as_table_like() {
            Some(sub) if item.as_value().is_none() || item.as_inline_table().is_some() => {
                flatten(sub, k, &mut settings)
            }
            _ => {
                if let Some(v) = toml_to_json(item) {
                    settings.insert(k.to_string(), v);
                }
            }
        }
    }
    let borrow = t
        .get("borrow")
        .and_then(Item::as_table_like)
        .map(|b| {
            b.iter()
                .filter_map(|(k, v)| {
                    let n: u8 = k.strip_prefix("slot")?.parse().ok()?;
                    Some((n, v.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SetEntry {
        id,
        created: t.get("created").and_then(Item::as_bool).unwrap_or(false),
        renamed_from: t
            .get("renamed_from")
            .and_then(Item::as_str)
            .map(str::to_string),
        add: strings(t.get("add")),
        remove: strings(t.get("remove")),
        borrow,
        shader: t
            .get("shader")
            .and_then(Item::as_str)
            .and_then(|s| serde_json::from_value(Json::String(s.to_string())).ok()),
        settings,
    })
}

impl SetsDoc {
    pub fn open(root: &Path) -> SetsDoc {
        let mut d = SetsDoc {
            path: root.join(FILE_NAME),
            doc: DocumentMut::new(),
            disk: None,
            dirty: false,
            error: None,
            rev: next_rev(),
        };
        d.reload();
        d
    }

    pub fn reload(&mut self) {
        self.rev = next_rev();
        self.dirty = false;
        self.error = None;
        self.doc = DocumentMut::new();
        self.disk = SourceStamp::of(&self.path);
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            Ok(text) => match text.parse::<DocumentMut>() {
                Ok(doc) => self.doc = doc,
                Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            },
        }
    }

    /// Changes on every edit, restore and reload (the set-view cache key).
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn read_only(&self) -> bool {
        self.error.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn changed_on_disk(&self) -> bool {
        SourceStamp::of(&self.path) != self.disk
    }

    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Replace the document with `text` (undo/redo): an unsaved change. Ignored while read-only.
    pub fn restore(&mut self, text: &str) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        if let Ok(doc) = text.parse::<DocumentMut>() {
            self.doc = doc;
            self.dirty = true;
        }
    }

    /// Undo/redo: `restore` that keeps the live `last_json_sha256`. The hash records the JSON on disk, which undo
    /// does not change, so it is outside the undoable state. When only the hash differs, nothing changes (and the
    /// document is not marked dirty).
    pub fn restore_keeping_json_sha(&mut self, text: &str) {
        self.rev = next_rev();
        if self.read_only() || self.text() == text {
            return;
        }
        let (doc, dirty, sha) = (self.doc.clone(), self.dirty, self.last_json_sha256());
        let old = self.text();
        self.restore(text);
        if self.last_json_sha256() != sha {
            self.set_project("last_json_sha256", sha.map(Value::from));
        }
        if self.text() == old {
            self.doc = doc;
            self.dirty = dirty;
        }
    }

    pub fn save(&mut self, force: bool) -> Result<(), SaveError> {
        if self.read_only() {
            return Err(SaveError::ReadOnly);
        }
        if !force && self.changed_on_disk() {
            return Err(SaveError::ChangedOnDisk);
        }
        let tmp = self.path.with_extension("toml.sdc-tmp");
        std::fs::write(&tmp, self.doc.to_string())
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .map_err(|e| SaveError::Io(e.to_string()))?;
        self.disk = SourceStamp::of(&self.path);
        self.dirty = false;
        Ok(())
    }

    fn project_str(&self, key: &str) -> Option<String> {
        self.doc
            .get("project")?
            .get(key)?
            .as_str()
            .map(str::to_string)
    }

    fn set_project(&mut self, key: &str, v: Option<Value>) {
        self.rev = next_rev();
        let absent = self.doc.get("project").and_then(|p| p.get(key)).is_none();
        if self.read_only() || (v.is_none() && absent) {
            return;
        }
        let Some(p) = sub_table(self.doc.as_table_mut(), "project", || {
            Item::Table(Table::new())
        }) else {
            return;
        };
        match v {
            Some(v) => set_value(p, key, v),
            None => {
                p.remove(key);
            }
        }
        self.dirty = true;
    }

    pub fn json_name(&self) -> Option<String> {
        self.project_str("json_name")
    }
    pub fn set_json_name(&mut self, name: Option<&str>) {
        self.rev = next_rev();
        self.set_project("json_name", name.map(Value::from));
    }
    pub fn last_json_sha256(&self) -> Option<String> {
        self.project_str("last_json_sha256")
    }
    pub fn set_last_json_sha256(&mut self, sha: &str) {
        self.rev = next_rev();
        self.set_project("last_json_sha256", Some(Value::from(sha)));
    }

    pub fn order(&self) -> Vec<String> {
        strings(self.doc.get("project").and_then(|p| p.get("order")))
    }
    pub fn set_order(&mut self, ids: &[String]) {
        self.rev = next_rev();
        let mut arr = Array::new();
        for id in ids {
            arr.push(id.as_str());
        }
        self.set_project("order", Some(Value::Array(arr)));
    }

    /// Files whose sub-type was switched to its PBR counterpart because their set's diffuse became `pbr_albedo`
    /// (so a revert finds them). GUI bookkeeping in `[project]`: `sdc.toml` rejects unknown keys.
    pub fn auto_pbr(&self) -> Vec<String> {
        strings(self.doc.get("project").and_then(|p| p.get("auto_pbr")))
    }
    pub fn set_auto_pbr(&mut self, keys: &[String]) {
        self.rev = next_rev();
        let mut arr = Array::new();
        for k in keys {
            arr.push(k.as_str());
        }
        self.set_project("auto_pbr", (!keys.is_empty()).then_some(Value::Array(arr)));
    }

    pub fn entries(&self) -> Vec<SetEntry> {
        self.doc
            .get("sets")
            .and_then(Item::as_array_of_tables)
            .map(|a| a.iter().filter_map(entry_of).collect())
            .unwrap_or_default()
    }

    fn tables_mut(&mut self) -> &mut ArrayOfTables {
        if !self.doc.get("sets").is_some_and(Item::is_array_of_tables) {
            self.doc
                .insert("sets", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        self.doc["sets"]
            .as_array_of_tables_mut()
            .expect("just made")
    }

    /// The table of set `id`, created at the end when missing (ids compare case-insensitively).
    fn table_mut(&mut self, id: &str) -> &mut Table {
        let lower = fold_id(id);
        let tables = self.tables_mut();
        let i = tables.iter().position(|t| {
            t.get("id")
                .and_then(Item::as_str)
                .is_some_and(|s| fold_id(s) == lower)
        });
        let i = match i {
            Some(i) => i,
            None => {
                let mut t = Table::new();
                t.insert("id", toml_edit::value(id));
                tables.push(t);
                tables.len() - 1
            }
        };
        tables.get_mut(i).expect("index in range")
    }

    pub fn create_set(&mut self, id: &str) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        self.table_mut(id).insert("created", toml_edit::value(true));
        self.dirty = true;
    }

    /// Drop set `id`'s entry and remove it from the order list (DECISIONS P2: its files return to automatic grouping).
    pub fn delete_set(&mut self, id: &str) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let lower = fold_id(id);
        if !self.entries().iter().any(|e| fold_id(&e.id) == lower) {
            return;
        }
        if let Some(tables) = self
            .doc
            .get_mut("sets")
            .and_then(Item::as_array_of_tables_mut)
        {
            tables.retain(|t| {
                !t.get("id")
                    .and_then(Item::as_str)
                    .is_some_and(|s| fold_id(s) == lower)
            });
        }
        let order: Vec<String> = self
            .order()
            .into_iter()
            .filter(|o| fold_id(o) != lower)
            .collect();
        if !self.order().is_empty() {
            self.set_order(&order);
        }
        self.dirty = true;
    }

    /// Rename set `from` to `to`. A created set just changes its id; an automatic set records `renamed_from`. Other
    /// entries that borrow from `from`, and the order list, follow.
    pub fn rename_set(&mut self, from: &str, to: &str) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let created = self
            .entries()
            .iter()
            .any(|e| same_id(&e.id, from) && e.created);
        let already = self
            .entries()
            .iter()
            .find(|e| same_id(&e.id, from))
            .and_then(|e| e.renamed_from.clone());
        let t = self.table_mut(from);
        t.insert("id", toml_edit::value(to));
        if !created {
            // Renamed back to the engine's own name: no rename left to apply.
            let original = already.unwrap_or_else(|| from.to_string());
            if same_id(&original, to) {
                t.remove("renamed_from");
            } else {
                t.insert("renamed_from", toml_edit::value(original));
            }
        }
        let order: Vec<String> = self
            .order()
            .into_iter()
            .map(|o| if same_id(&o, from) { to.to_string() } else { o })
            .collect();
        if !order.is_empty() {
            self.set_order(&order);
        }
        for e in self.entries() {
            for (slot, src) in &e.borrow {
                if same_id(src, from) {
                    self.set_borrow(&e.id, *slot, Some(to));
                }
            }
        }
        self.dirty = true;
    }

    fn list_edit(&mut self, id: &str, key: &str, file: &str, add: bool) {
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        let mut items = strings(t.get(key));
        items.retain(|f| !same_id(f, file));
        if add {
            items.push(file.to_string());
        }
        if items.is_empty() {
            t.remove(key);
        } else {
            let mut arr = Array::new();
            for f in &items {
                arr.push(f.as_str());
            }
            t.insert(key, toml_edit::value(arr));
        }
        self.dirty = true;
    }

    /// `file` moves into set `id` (a `Move` edit); it is taken out of every other entry's `add` and `remove`.
    pub fn add_member(&mut self, id: &str, file: &str) {
        self.rev = next_rev();
        for e in self.entries() {
            if !same_id(&e.id, id) {
                if e.add.iter().any(|f| same_id(f, file)) {
                    self.list_edit(&e.id, "add", file, false);
                }
                if e.remove.iter().any(|f| same_id(f, file)) {
                    self.list_edit(&e.id, "remove", file, false);
                }
            }
        }
        self.list_edit(id, "add", file, true);
    }

    /// `file` leaves set `id`: it is dropped from `id`'s `add` when it was moved in, else recorded in `remove`.
    pub fn remove_member(&mut self, id: &str, file: &str) {
        self.rev = next_rev();
        let moved_in = self
            .entries()
            .iter()
            .any(|e| same_id(&e.id, id) && e.add.iter().any(|f| same_id(f, file)));
        if moved_in {
            self.list_edit(id, "add", file, false);
        } else {
            self.list_edit(id, "remove", file, true);
        }
    }

    pub fn set_borrow(&mut self, id: &str, slot: u8, from: Option<&str>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, "borrow", || {
            Item::Value(Value::InlineTable(Default::default()))
        }) else {
            return;
        };
        let key = format!("slot{slot}");
        match from {
            Some(f) => set_value(b, &key, Value::from(f)),
            None => {
                b.remove(&key);
            }
        }
        if t.get("borrow")
            .and_then(Item::as_table_like)
            .is_some_and(TableLike::is_empty)
        {
            t.remove("borrow");
        }
        self.dirty = true;
    }

    /// Set (or with `None` remove) set `id`'s explicit shader (M6 spec §7).
    pub fn set_shader(&mut self, id: &str, shader: Option<engine::shader::Shader>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        // Clearing a set with no table changes nothing (no table, no dirty, no undo step).
        let lower = fold_id(id);
        let has_table = self
            .doc
            .get("sets")
            .and_then(Item::as_array_of_tables)
            .is_some_and(|ts| {
                ts.iter().any(|t| {
                    t.get("id")
                        .and_then(Item::as_str)
                        .is_some_and(|s| fold_id(s) == lower)
                })
            });
        if shader.is_none() && !has_table {
            return;
        }
        let t = self.table_mut(id);
        match shader {
            Some(s) => {
                t.insert("shader", toml_edit::value(s.id()));
            }
            None => {
                t.remove("shader");
            }
        }
        self.dirty = true;
    }

    /// Set (or with `None` remove) JSON setting `key` (a dotted schema path) of set `id`.
    pub fn set_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        if write_path(t, key, v) {
            self.dirty = true;
        }
    }
}

/// Write `v` at dotted `path` under `t` (sub-tables as dotted keys), or remove it with `None` (and any sub-table it
/// leaves empty). True when something changed.
fn write_path(t: &mut dyn TableLike, path: &str, v: Option<Json>) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    let (last, dirs) = parts.split_last().expect("one part at least");
    match v {
        Some(v) => {
            let Some(v) = json_to_toml(&v) else {
                return false;
            };
            let mut cur: &mut dyn TableLike = t;
            for d in dirs {
                match sub_table(cur, d, dotted_table) {
                    Some(next) => cur = next,
                    None => return false,
                }
            }
            set_value(cur, last, v);
            true
        }
        None => {
            fn remove(t: &mut dyn TableLike, parts: &[&str]) -> bool {
                if parts.len() == 1 {
                    return t.remove(parts[0]).is_some();
                }
                let Some(sub) = t.get_mut(parts[0]).and_then(Item::as_table_like_mut) else {
                    return false;
                };
                let hit = remove(sub, &parts[1..]);
                if sub.is_empty() {
                    t.remove(parts[0]);
                }
                hit
            }
            remove(t, &parts)
        }
    }
}
