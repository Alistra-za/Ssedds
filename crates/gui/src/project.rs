//! `sdc.toml` as the GUI edits it (M3a spec §5.3–5.4): format-preserving (`toml_edit`), so comments, ordering and
//! hand-written tables survive. The GUI owns only `target`, `[files.*]` and `defaults.packing.fill.*`.
use engine::codes::Code;
use engine::config::{RawMap, Target};
use engine::policy::SourceStamp;
use engine::presets::ConfigFile;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

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
                f.write_str("sdc.toml is read-only until its error is fixed and it is reloaded")
            }
            SaveError::ChangedOnDisk => f.write_str("sdc.toml changed on disk since it was loaded"),
            SaveError::Io(e) => write!(f, "could not save sdc.toml: {e}"),
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

fn json_to_toml(v: &Json) -> Option<Value> {
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

fn toml_to_json(v: &Item) -> Option<Json> {
    let text = match v {
        Item::Value(v) => format!("v = {v}"),
        _ => return None,
    };
    let t: toml::Table = toml::from_str(&text).ok()?;
    serde_json::to_value(&t["v"]).ok()
}

/// A new table whose keys print as dotted keys (`alpha.policy = ...`) inside their parent.
fn dotted_table() -> Item {
    let mut t = Table::new();
    t.set_dotted(true);
    Item::Table(t)
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
                    if let Err(e) = self.config() {
                        self.error = Some(format!("{}: {e}", self.path.display()));
                        self.doc = DocumentMut::new();
                    }
                }
                Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            },
        }
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

    /// The document as the engine parses it.
    pub fn config(&self) -> Result<ConfigFile, String> {
        toml::from_str(&self.doc.to_string()).map_err(|e| e.to_string())
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
            self.doc["target"] = toml_edit::value(s);
            self.dirty = true;
        }
    }

    /// The `[files]` table, created implicit (no bare `[files]` header) when missing.
    fn files_mut(&mut self) -> &mut Table {
        let item = self.doc.entry("files").or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        item.as_table_mut().expect("[files] is a table")
    }

    /// The entry for `key`: `[files."<key>"]`, with dotted keys inside for nested fields.
    fn entry_mut(&mut self, key: &str) -> &mut Table {
        let files = self.files_mut();
        if !files.contains_key(key) {
            files.insert(key, Item::Table(Table::new()));
        }
        files[key]
            .as_table_mut()
            .expect("a [files] entry is a table")
    }

    fn entry(&self, key: &str) -> Option<&Table> {
        self.doc.get("files")?.as_table()?.get(key)?.as_table()
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
        let mut t = self.entry_mut(key);
        for part in path {
            if !t.contains_key(part) {
                t.insert(part, dotted_table());
            }
            let Some(sub) = t[*part].as_table_mut() else {
                return;
            };
            t = sub;
        }
        t[*last] = Item::Value(v);
        self.dirty = true;
    }

    pub fn clear_file_value(&mut self, key: &str, field: &str) {
        if self.read_only() || self.entry(key).is_none() {
            return;
        }
        fn remove(t: &mut Table, parts: &[&str]) -> bool {
            if parts.len() == 1 {
                return t.remove(parts[0]).is_some();
            }
            let Some(sub) = t.get_mut(parts[0]).and_then(Item::as_table_mut) else {
                return false;
            };
            let hit = remove(sub, &parts[1..]);
            if sub.is_empty() {
                t.remove(parts[0]);
            }
            hit
        }
        let parts: Vec<&str> = field.split('.').collect();
        let entry = self.entry_mut(key);
        if remove(entry, &parts) {
            self.dirty = true;
        }
        if self.entry(key).is_some_and(Table::is_empty) {
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

    /// `defaults.packing.fill.<id> = v`, creating missing tables (dotted keys inside an existing `[defaults]`).
    fn write_fill(&mut self, id: &str, v: u8) -> Option<()> {
        let defaults = self
            .doc
            .entry("defaults")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_mut()?;
        let packing = defaults
            .entry("packing")
            .or_insert_with(dotted_table)
            .as_table_mut()?;
        let fill = packing
            .entry("fill")
            .or_insert_with(dotted_table)
            .as_table_mut()?;
        fill[id] = toml_edit::value(i64::from(v));
        Some(())
    }

    /// Remove `defaults.packing.fill.<id>`, and the tables that leaves empty (no bare `[defaults]` header is left
    /// behind). `None` when there was nothing to remove.
    fn remove_fill(&mut self, id: &str) -> Option<()> {
        let defaults = self.doc.get_mut("defaults")?.as_table_mut()?;
        let packing = defaults.get_mut("packing")?.as_table_mut()?;
        let fill = packing.get_mut("fill")?.as_table_mut()?;
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
        self.doc
            .get("files")
            .and_then(Item::as_table)
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default()
    }

    pub fn remove_file_entry(&mut self, key: &str) {
        if self.read_only() {
            return;
        }
        if self.files_mut().remove(key).is_some() {
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
