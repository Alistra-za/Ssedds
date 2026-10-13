//! `sdc.toml` as the GUI edits it (M3a spec §5.3–5.4): format-preserving (`toml_edit`), so comments, ordering and
//! hand-written tables survive. The GUI owns only `target`, `[files.*]` and `defaults.packing.fill.*`.
use engine::codes::Code;
use engine::config::{RawMap, Target};
use engine::policy::SourceStamp;
use engine::presets::ConfigFile;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// The file did not parse when it was opened; the GUI does not write it.
    ReadOnly,
    /// The file changed on disk since it was loaded or saved; save again with `force` to overwrite.
    ChangedOnDisk,
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::ReadOnly => {
                f.write_str("the file is read-only until its error is fixed and it is reloaded")
            }
            SaveError::ChangedOnDisk => f.write_str("the file changed on disk since it was loaded"),
            SaveError::Io(e) => write!(f, "could not save: {e}"),
        }
    }
}

pub struct ProjectDoc {
    path: PathBuf,
    doc: DocumentMut,
    disk: Option<SourceStamp>,
    dirty: bool,
    error: Option<String>,
}

pub(crate) fn json_to_toml(v: &Json) -> Option<Value> {
    Some(match v {
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::from(i),
            None => Value::from(n.as_f64()?),
        },
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(a) => {
            let mut arr = Array::new();
            for x in a {
                arr.push(json_to_toml(x)?);
            }
            Value::Array(arr)
        }
        Json::Null | Json::Object(_) => return None,
    })
}

pub(crate) fn toml_to_json(v: &Item) -> Option<Json> {
    let text = match v {
        Item::Value(v) => format!("v = {v}"),
        _ => return None,
    };
    let t: toml::Table = toml::from_str(&text).ok()?;
    serde_json::to_value(&t["v"]).ok()
}

/// A new table whose keys print as dotted keys (`alpha.policy = ...`) inside their parent.
pub(crate) fn dotted_table() -> Item {
    let mut t = Table::new();
    t.set_dotted(true);
    Item::Table(t)
}

/// `t[name]` as a table or inline table, created with `make()` when missing. `None` when it exists but is a
/// plain value. (`insert` rather than `entry`: inserting a `Table` item into an inline table converts it.)
pub(crate) fn sub_table<'a>(
    t: &'a mut dyn TableLike,
    name: &str,
    make: impl FnOnce() -> Item,
) -> Option<&'a mut dyn TableLike> {
    if !t.contains_key(name) {
        t.insert(name, make());
    }
    t.get_mut(name)?.as_table_like_mut()
}

/// Set `t[key] = v`, replacing an existing item in place so the key keeps its comments and position; a comment on
/// the value's own line (its decor) is carried over (audit 19).
pub(crate) fn set_value(t: &mut dyn TableLike, key: &str, mut v: Value) {
    match t.get_mut(key) {
        Some(slot) => {
            if let Some(old) = slot.as_value() {
                *v.decor_mut() = old.decor().clone();
            }
            *slot = Item::Value(v);
        }
        None => {
            t.insert(key, Item::Value(v));
        }
    }
}

/// The spelling `key` has in `t`: an exact match, else a case-insensitive one (the engine matches `[files]` keys
/// case-insensitively, so a GUI write must reuse a hand-written `Landscape/Rock01_N.png` rather than add a twin).
pub(crate) fn find_key(t: &dyn TableLike, key: &str) -> Option<String> {
    if t.contains_key(key) {
        return Some(key.to_owned());
    }
    let lower = key.to_lowercase();
    t.iter()
        .map(|(k, _)| k)
        .find(|k| k.to_lowercase() == lower)
        .map(str::to_owned)
}

impl ProjectDoc {
    pub fn open(root: &Path) -> ProjectDoc {
        let path = root.join("sdc.toml");
        let mut d = ProjectDoc {
            path,
            doc: DocumentMut::new(),
            disk: None,
            dirty: false,
            error: None,
        };
        d.reload();
        d
    }

    /// Re-read the file, dropping unsaved edits.
    pub fn reload(&mut self) {
        self.dirty = false;
        self.error = None;
        self.doc = DocumentMut::new();
        self.disk = SourceStamp::of(&self.path);
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            Ok(text) => match text.parse::<DocumentMut>() {
                Ok(doc) => {
                    self.doc = doc;
                    // The engine's own up-front checks on `[files]` (unknown field or code) are parse errors too.
                    let checked = self
                        .config()
                        .and_then(|c| c.file_entries().map(|_| ()).map_err(|e| e.to_string()));
                    if let Err(e) = checked {
                        self.set_error(&e);
                    }
                }
                Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            },
        }
    }

    /// Mark the file unusable (M3a spec §7.4): read-only with `message`, its contents and unsaved edits dropped. Used
    /// when the engine rejects a config that parsed; `reload` clears it.
    pub fn set_error(&mut self, message: &str) {
        self.error = Some(format!("{}: {message}", self.path.display()));
        self.doc = DocumentMut::new();
        self.dirty = false;
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

    /// The document as text (undo snapshots, DECISIONS P9).
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Replace the document with `text` (an undo or redo). It is an unsaved change. A read-only document is left
    /// alone: its edits never happened.
    pub fn restore(&mut self, text: &str) {
        if self.read_only() {
            return;
        }
        if let Ok(doc) = text.parse::<DocumentMut>() {
            self.doc = doc;
            self.dirty = true;
        }
    }

    /// The document as the engine parses it (migration hook included, spec §5).
    pub fn config(&self) -> Result<ConfigFile, String> {
        ConfigFile::parse(&self.doc.to_string(), "sdc.toml").map_err(|e| e.to_string())
    }

    pub fn target(&self) -> Option<Target> {
        self.config().ok()?.target
    }
    pub fn set_target(&mut self, t: Target) {
        if self.read_only() {
            return;
        }
        let s = serde_json::to_value(t).ok();
        if let Some(Json::String(s)) = s {
            set_value(self.doc.as_table_mut(), "target", Value::from(s));
            self.dirty = true;
        }
    }

    /// The `[files]` table (or inline table), if there is one.
    fn files(&self) -> Option<&dyn TableLike> {
        self.doc.get("files")?.as_table_like()
    }

    /// The entry for `key`, matched the way the engine matches (case-insensitively), as a table or inline table.
    fn entry(&self, key: &str) -> Option<&dyn TableLike> {
        let files = self.files()?;
        files.get(&find_key(files, key)?)?.as_table_like()
    }

    /// The entry for `key`, created as `[files."<key>"]` when no spelling of it exists yet. `None` when `[files]`
    /// or the entry is not a table.
    fn entry_mut(&mut self, key: &str) -> Option<&mut dyn TableLike> {
        // `[files]` is created implicit (no bare `[files]` header).
        let files = sub_table(self.doc.as_table_mut(), "files", || {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })?;
        let name = find_key(files, key).unwrap_or_else(|| key.to_owned());
        sub_table(files, &name, || Item::Table(Table::new()))
    }

    /// Remove the entry for `key` (any spelling of it); true when one existed.
    fn drop_entry(&mut self, key: &str) -> bool {
        let Some(files) = self.doc.get_mut("files").and_then(Item::as_table_like_mut) else {
            return false;
        };
        let Some(name) = find_key(files, key) else {
            return false;
        };
        files.remove(&name).is_some()
    }

    pub fn file_value(&self, key: &str, field: &str) -> Option<Json> {
        let mut item: &Item = self.entry(key)?.get(field.split('.').next()?)?;
        for part in field.split('.').skip(1) {
            item = item.get(part)?;
        }
        toml_to_json(item)
    }

    pub fn set_file_value(&mut self, key: &str, field: &str, value: &Json) {
        if self.read_only() {
            return;
        }
        let Some(v) = json_to_toml(value) else { return };
        let parts: Vec<&str> = field.split('.').collect();
        let (last, path) = parts.split_last().expect("split yields one part at least");
        let written = (|| {
            let mut t = self.entry_mut(key)?;
            for part in path {
                t = sub_table(t, part, dotted_table)?;
            }
            set_value(t, last, v);
            Some(())
        })();
        if written.is_some() {
            self.dirty = true;
        } else if self.entry(key).is_some_and(|t| t.is_empty()) {
            // A field path ran into a non-table: do not leave the entry we just created behind.
            self.drop_entry(key);
        }
    }

    pub fn clear_file_value(&mut self, key: &str, field: &str) {
        if self.read_only() || self.entry(key).is_none() {
            return;
        }
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
        let parts: Vec<&str> = field.split('.').collect();
        if self
            .entry_mut(key)
            .is_some_and(|entry| remove(entry, &parts))
        {
            self.dirty = true;
        }
        if self.entry(key).is_some_and(|t| t.is_empty()) {
            self.remove_file_entry(key);
        }
    }

    pub fn allow(&self, key: &str) -> BTreeSet<Code> {
        self.file_value(key, "allow")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| c.as_str().and_then(Code::parse))
            .collect()
    }

    pub fn set_allow(&mut self, key: &str, codes: &BTreeSet<Code>) {
        if codes.is_empty() {
            self.clear_file_value(key, "allow");
        } else {
            let list: Vec<Json> = codes.iter().map(|c| Json::from(c.as_str())).collect();
            self.set_file_value(key, "allow", &Json::Array(list));
        }
    }

    pub fn fill(&self, role: RawMap) -> Option<u8> {
        let v = self
            .doc
            .get("defaults")?
            .get("packing")?
            .get("fill")?
            .get(role.id())?;
        toml_to_json(v)?.as_u64().and_then(|n| u8::try_from(n).ok())
    }

    pub fn set_fill(&mut self, role: RawMap, value: Option<u8>) {
        if self.read_only() {
            return;
        }
        let id = role.id();
        let changed = match value {
            Some(v) => self.write_fill(&id, v).is_some(),
            None => self.remove_fill(&id).is_some(),
        };
        self.dirty |= changed;
    }

    /// `defaults.packing.fill.<id> = v`, creating missing tables (dotted keys inside an existing `[defaults]`);
    /// works through inline tables. `None` when a table on the path is not a table.
    fn write_fill(&mut self, id: &str, v: u8) -> Option<()> {
        let defaults = sub_table(self.doc.as_table_mut(), "defaults", || {
            Item::Table(Table::new())
        })?;
        let packing = sub_table(defaults, "packing", dotted_table)?;
        let fill = sub_table(packing, "fill", dotted_table)?;
        set_value(fill, id, Value::from(i64::from(v)));
        Some(())
    }

    /// Remove `defaults.packing.fill.<id>`, and the tables that leaves empty (no bare `[defaults]` header is left
    /// behind). `None` when there was nothing to remove.
    fn remove_fill(&mut self, id: &str) -> Option<()> {
        let defaults = self.doc.get_mut("defaults")?.as_table_like_mut()?;
        let packing = defaults.get_mut("packing")?.as_table_like_mut()?;
        let fill = packing.get_mut("fill")?.as_table_like_mut()?;
        fill.remove(id)?;
        if fill.is_empty() {
            packing.remove("fill");
        }
        if packing.is_empty() {
            defaults.remove("packing");
        }
        if defaults.is_empty() {
            self.doc.remove("defaults");
        }
        Some(())
    }

    pub fn file_keys(&self) -> Vec<String> {
        self.files()
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default()
    }

    pub fn remove_file_entry(&mut self, key: &str) {
        if self.read_only() {
            return;
        }
        if self.drop_entry(key) {
            self.dirty = true;
        }
    }

    pub fn changed_on_disk(&self) -> bool {
        SourceStamp::of(&self.path) != self.disk
    }

    /// Write the document (atomically: a temporary file, then rename).
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
}
