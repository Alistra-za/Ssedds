//! `sdc-project.toml` (M3b spec §7.1): the user's sets and JSON settings, next to `sdc.toml`. Format-preserving
//! like `ProjectDoc`; written only on Save or Export. A file that does not parse makes set and JSON editing
//! read-only (DECISIONS P8); `sdc.toml` editing is unaffected.
use crate::model::cm_json::{block_of as cm_block_of, CmBlock};
use crate::model::cs_pbr::{self, block_of, field, fields, CsBlock, Kind, Template};
use crate::pgp::settings::Settings;
use crate::project::{
    dotted_table, json_to_toml, set_value, sub_table, toml_to_json, SaveError, TomlFile,
};
use engine::sets::{fold_id, same_id};
use serde_json::Value as Json;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use toml_edit::{Array, ArrayOfTables, Item, Table, TableLike, Value};

pub const FILE_NAME: &str = "sdc-project.toml";
/// GUI tweaks G4: the array of extra PGPatcher entries (delete sections).
const SECTIONS: &str = "pgp_sections";
/// Design §5: `[project] cs_pbr_sha256`, path under the export root → hash of the CS JSON sdc last wrote there.
const CS_SHA: &str = "cs_pbr_sha256";
/// Complex material design §3: `[project] cm_json_sha256`, path under the export root → hash of the
/// `<texture>.json` sdc last wrote there.
const CM_SHA: &str = "cm_json_sha256";
/// Keys of a `[[sets]]` table that are not JSON settings.
const RESERVED: [&str; 13] = [
    "id",
    "created",
    "renamed_from",
    "add",
    "remove",
    "borrow",
    "shader",
    "lock",
    "output_name",
    "cs_txst",
    "cs_mato",
    "skin",
    "cm",
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
    /// F3a Task 8: locked slot numbers (`lock = ["slot2"]`), kept as they are by drops, slot commands and bulk edits.
    pub lock: BTreeSet<u8>,
    /// F3b D2: the base name every output of the set takes (`output_name = "steel"`): `steel_n.dds`, `steel.dds`…
    /// in each output's own folder. Never on a CS PBR set, which renames through the PGPatcher `rename` key.
    pub output_name: Option<String>,
    /// Design §3: the Community Shaders PBR texture-set and material-object blocks (`[sets.cs_txst]`, `[sets.cs_mato]`).
    pub cs_txst: Option<CsBlock>,
    pub cs_mato: Option<CsBlock>,
    /// Complex material design §3: the PGPatcher complex material JSON block (`[sets.cm]`).
    pub cm: Option<CmBlock>,
    /// Task 10b: the Advanced skin card's explicit values (`skin.<CS key>`). Not PGPatcher settings; unknown keys stay.
    pub skin: Settings,
    pub settings: Settings,
}

/// Source of `SetsDoc::rev`: one counter for every document, so a replaced document never reuses a revision (DECISIONS Note Y7).
static NEXT_REV: AtomicU64 = AtomicU64::new(1);
pub(crate) fn next_rev() -> u64 {
    NEXT_REV.fetch_add(1, Ordering::Relaxed)
}

pub struct SetsDoc {
    f: TomlFile,
    entries_cache: RefCell<Option<(u64, Arc<Vec<SetEntry>>)>>,
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
    let block = |k: Kind| {
        t.get(k.key())
            .and_then(Item::as_table_like)
            .map(|b| block_of(b, k))
    };
    let (cs_txst, cs_mato) = (block(Kind::Txst), block(Kind::Mato));
    if let Some(b) = &cs_txst {
        cs_pbr::overlay(&mut settings, b);
    }
    let mut skin = Settings::new();
    if let Some(sub) = t.get("skin").and_then(Item::as_table_like) {
        flatten(sub, "", &mut skin);
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
        lock: strings(t.get("lock"))
            .iter()
            .filter_map(|s| s.strip_prefix("slot")?.parse().ok())
            .collect(),
        output_name: t
            .get("output_name")
            .and_then(Item::as_str)
            .map(str::to_string)
            .filter(|n| !n.is_empty()),
        cs_txst,
        cs_mato,
        cm: t.get("cm").and_then(Item::as_table_like).map(cm_block_of),
        skin,
        settings,
    })
}

impl SetsDoc {
    pub fn open(root: &Path) -> SetsDoc {
        SetsDoc {
            f: TomlFile::open(root.join(FILE_NAME)),
            entries_cache: RefCell::default(),
        }
    }

    pub fn reload(&mut self) {
        self.f.reload();
    }

    /// Changes on every edit, restore and reload (the set-view cache key).
    pub fn rev(&self) -> u64 {
        self.f.rev
    }

    pub fn path(&self) -> &Path {
        &self.f.path
    }
    pub fn is_dirty(&self) -> bool {
        self.f.dirty
    }
    pub fn read_only(&self) -> bool {
        self.f.read_only()
    }
    pub fn error(&self) -> Option<&str> {
        self.f.error.as_deref()
    }
    pub fn changed_on_disk(&self) -> bool {
        self.f.changed_on_disk()
    }

    pub fn text(&self) -> String {
        self.f.text()
    }

    /// Replace the document with `text` (undo/redo): an unsaved change. Ignored while read-only.
    pub fn restore(&mut self, text: &str) {
        self.f.restore(text);
    }

    /// Undo/redo: `restore` that keeps the live `last_json_sha256`, `cs_pbr_sha256` and `cm_json_sha256`. The hash records the JSON on disk, which undo
    /// does not change, so it is outside the undoable state. When only the hash differs, nothing changes (and the
    /// document is not marked dirty).
    pub fn restore_keeping_json_sha(&mut self, text: &str) {
        if self.read_only() || self.text() == text {
            return;
        }
        let (doc, dirty, sha) = (self.f.doc.clone(), self.f.dirty, self.last_json_sha256());
        let cs = self.cs_pbr_shas();
        let cm = self.cm_json_shas();
        let old = self.text();
        self.restore(text);
        if self.last_json_sha256() != sha {
            self.set_project("last_json_sha256", sha.map(Value::from));
        }
        if self.cs_pbr_shas() != cs {
            self.set_cs_pbr_shas(&cs);
        }
        if self.cm_json_shas() != cm {
            self.set_cm_json_shas(&cm);
        }
        if self.text() == old {
            self.f.doc = doc;
            self.f.dirty = dirty;
        }
    }

    pub fn save(&mut self, force: bool) -> Result<(), SaveError> {
        self.f.save(force)
    }

    fn project_str(&self, key: &str) -> Option<String> {
        self.f
            .doc
            .get("project")?
            .get(key)?
            .as_str()
            .map(str::to_string)
    }

    fn set_project(&mut self, key: &str, v: Option<Value>) {
        let absent = self.f.doc.get("project").and_then(|p| p.get(key)).is_none();
        if self.read_only() || (v.is_none() && absent) {
            return;
        }
        let Some(p) = sub_table(self.f.doc.as_table_mut(), "project", || {
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
        self.f.touch();
    }

    pub fn json_name(&self) -> Option<String> {
        self.project_str("json_name")
    }
    pub fn set_json_name(&mut self, name: Option<&str>) {
        self.set_project("json_name", name.map(Value::from));
    }
    pub fn last_json_sha256(&self) -> Option<String> {
        self.project_str("last_json_sha256")
    }
    pub fn set_last_json_sha256(&mut self, sha: &str) {
        self.set_project("last_json_sha256", Some(Value::from(sha)));
    }

    fn shas(&self, key: &str) -> BTreeMap<String, String> {
        self.f
            .doc
            .get("project")
            .and_then(|p| p.get(key))
            .and_then(Item::as_table_like)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| Some((k.to_string(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_shas(&mut self, key: &str, shas: &BTreeMap<String, String>) {
        let mut t = toml_edit::InlineTable::new();
        for (k, v) in shas {
            t.insert(k, Value::from(v.as_str()));
        }
        self.set_project(key, (!shas.is_empty()).then_some(Value::InlineTable(t)));
    }

    pub fn cs_pbr_shas(&self) -> BTreeMap<String, String> {
        self.shas(CS_SHA)
    }

    pub fn set_cs_pbr_shas(&mut self, shas: &BTreeMap<String, String>) {
        self.set_shas(CS_SHA, shas)
    }

    pub fn cm_json_shas(&self) -> BTreeMap<String, String> {
        self.shas(CM_SHA)
    }

    pub fn set_cm_json_shas(&mut self, shas: &BTreeMap<String, String>) {
        self.set_shas(CM_SHA, shas)
    }

    pub fn order(&self) -> Vec<String> {
        strings(self.f.doc.get("project").and_then(|p| p.get("order")))
    }
    pub fn set_order(&mut self, ids: &[String]) {
        let mut arr = Array::new();
        for id in ids {
            arr.push(id.as_str());
        }
        self.set_project("order", Some(Value::Array(arr)));
    }

    /// Files whose sub-type was switched to its PBR counterpart because their set's diffuse became `pbr_albedo`
    /// (so a revert finds them). GUI bookkeeping in `[project]`: `sdc.toml` rejects unknown keys.
    pub fn auto_pbr(&self) -> Vec<String> {
        strings(self.f.doc.get("project").and_then(|p| p.get("auto_pbr")))
    }
    pub fn set_auto_pbr(&mut self, keys: &[String]) {
        let mut arr = Array::new();
        for k in keys {
            arr.push(k.as_str());
        }
        self.set_project("auto_pbr", (!keys.is_empty()).then_some(Value::Array(arr)));
    }

    /// The `[[sets]]` entries, parsed once per revision (REPORT #9).
    pub fn entries(&self) -> Arc<Vec<SetEntry>> {
        let rev = self.f.rev;
        if let Some((r, e)) = &*self.entries_cache.borrow() {
            if *r == rev {
                return e.clone();
            }
        }
        let e = Arc::new(self.parse_entries());
        *self.entries_cache.borrow_mut() = Some((rev, e.clone()));
        e
    }

    /// The entries as the document holds them now. Mutators use this: mid-edit the revision is not bumped yet.
    fn parse_entries(&self) -> Vec<SetEntry> {
        self.f
            .doc
            .get("sets")
            .and_then(Item::as_array_of_tables)
            .map(|a| a.iter().filter_map(entry_of).collect())
            .unwrap_or_default()
    }

    /// GUI tweaks G4: the `[[pgp_sections]]` tables, in file order: extra PGPatcher entries that belong to no set
    /// (delete entries). Each is its JSON keys, flattened like a set's settings.
    pub fn sections(&self) -> Vec<Settings> {
        self.f
            .doc
            .get(SECTIONS)
            .and_then(Item::as_array_of_tables)
            .map(|a| {
                a.iter()
                    .map(|t| {
                        let mut out = Settings::new();
                        flatten(t, "", &mut out);
                        out
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Append a section with `keys` (values JSON can hold; others are left out).
    pub fn add_section(&mut self, keys: &Settings) {
        if self.read_only() {
            return;
        }
        if !self
            .f
            .doc
            .get(SECTIONS)
            .is_some_and(Item::is_array_of_tables)
        {
            self.f
                .doc
                .insert(SECTIONS, Item::ArrayOfTables(ArrayOfTables::new()));
        }
        let mut t = Table::new();
        for (k, v) in keys {
            write_path(&mut t, k, Some(v.clone()));
        }
        self.f.doc[SECTIONS]
            .as_array_of_tables_mut()
            .expect("just made")
            .push(t);
        self.f.touch();
    }

    /// Remove section `i` (the array goes when it is empty).
    pub fn remove_section(&mut self, i: usize) {
        if self.read_only() {
            return;
        }
        let Some(a) = self
            .f
            .doc
            .get_mut(SECTIONS)
            .and_then(Item::as_array_of_tables_mut)
        else {
            return;
        };
        if i < a.len() {
            a.remove(i);
            if a.is_empty() {
                self.f.doc.remove(SECTIONS);
            }
            self.f.touch();
        }
    }

    /// Set (or with `None` remove) key `key` of section `i`.
    pub fn set_section_value(&mut self, i: usize, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        let Some(t) = self
            .f
            .doc
            .get_mut(SECTIONS)
            .and_then(Item::as_array_of_tables_mut)
            .and_then(|a| a.get_mut(i))
        else {
            return;
        };
        if write_path(t, key, v) {
            self.f.touch();
        }
    }

    fn tables_mut(&mut self) -> &mut ArrayOfTables {
        if !self.f.doc.get("sets").is_some_and(Item::is_array_of_tables) {
            self.f
                .doc
                .insert("sets", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        self.f.doc["sets"]
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
                // A new entry even when the caller writes nothing into it (no dirty): the entries changed.
                let i = tables.len() - 1;
                self.f.rev = next_rev();
                i
            }
        };
        self.tables_mut().get_mut(i).expect("index in range")
    }

    pub fn create_set(&mut self, id: &str) {
        if self.read_only() {
            return;
        }
        self.table_mut(id).insert("created", toml_edit::value(true));
        self.f.touch();
    }

    /// Drop set `id`'s entry and remove it from the order list (DECISIONS P2: its files return to automatic grouping).
    pub fn delete_set(&mut self, id: &str) {
        if self.read_only() {
            return;
        }
        let lower = fold_id(id);
        if !self.parse_entries().iter().any(|e| fold_id(&e.id) == lower) {
            return;
        }
        if let Some(tables) = self
            .f
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
        self.f.touch();
    }

    /// Rename set `from` to `to`. A created set just changes its id; an automatic set records `renamed_from`. Other
    /// entries that borrow from `from`, and the order list, follow.
    pub fn rename_set(&mut self, from: &str, to: &str) {
        if self.read_only() {
            return;
        }
        let created = self
            .parse_entries()
            .iter()
            .any(|e| same_id(&e.id, from) && e.created);
        let already = self
            .parse_entries()
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
        for e in self.parse_entries() {
            for (slot, src) in &e.borrow {
                if same_id(src, from) {
                    self.set_borrow(&e.id, *slot, Some(to));
                }
            }
        }
        self.f.touch();
    }

    /// GUI tweaks G1: the automatic set `from` is now named `to` by the engine (its members were given new input
    /// names). The entry that applies to it follows: a renamed entry (`renamed_from = from`) gets
    /// `renamed_from = to`; an entry under the automatic id itself takes the new id, with the order list and other
    /// entries' borrows. Created sets are never touched.
    pub fn retarget_auto(&mut self, from: &str, to: &str) {
        if self.read_only() || same_id(from, to) {
            return;
        }
        let entries = self.parse_entries();
        if let Some(e) = entries
            .iter()
            .find(|e| !e.created && e.renamed_from.as_deref().is_some_and(|r| same_id(r, from)))
        {
            let id = e.id.clone();
            let t = self.table_mut(&id);
            if same_id(&id, to) {
                t.remove("renamed_from");
            } else {
                t.insert("renamed_from", toml_edit::value(to));
            }
            self.f.touch();
        } else if entries
            .iter()
            .any(|e| !e.created && e.renamed_from.is_none() && same_id(&e.id, from))
            // Never two entries under one id: an entry already at `to` keeps this one where it is.
            && !entries.iter().any(|e| same_id(&e.id, to))
        {
            self.rename_set(from, to);
            self.table_mut(to).remove("renamed_from");
            self.f.touch();
        }
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
        self.f.touch();
    }

    /// `file` moves into set `id` (a `Move` edit); it is taken out of every other entry's `add` and `remove`.
    pub fn add_member(&mut self, id: &str, file: &str) {
        for e in self.parse_entries() {
            if !same_id(&e.id, id) {
                if e.add.iter().any(|f| same_id(f, file)) {
                    self.list_edit(&e.id, "add", file, false);
                }
                if e.remove.iter().any(|f| same_id(f, file)) {
                    self.list_edit(&e.id, "remove", file, false);
                }
            }
        }
        // A file this set removed belongs to it by its name: lifting the remove brings it back (an `add` beside the
        // `remove` would leave it out, AG100).
        let removed_here = self
            .parse_entries()
            .iter()
            .any(|e| same_id(&e.id, id) && e.remove.iter().any(|f| same_id(f, file)));
        if removed_here {
            self.list_edit(id, "remove", file, false);
        } else {
            self.list_edit(id, "add", file, true);
        }
    }

    /// `file` leaves set `id`: it is dropped from `id`'s `add` when it was moved in, else recorded in `remove`.
    pub fn remove_member(&mut self, id: &str, file: &str) {
        let moved_in = self
            .parse_entries()
            .iter()
            .any(|e| same_id(&e.id, id) && e.add.iter().any(|f| same_id(f, file)));
        if moved_in {
            self.list_edit(id, "add", file, false);
        } else {
            self.list_edit(id, "remove", file, true);
        }
    }

    pub fn set_borrow(&mut self, id: &str, slot: u8, from: Option<&str>) {
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
        self.f.touch();
    }

    /// F3a Task 8: lock (or unlock) slot `slot` of set `id`.
    pub fn set_lock(&mut self, id: &str, slot: u8, on: bool) {
        self.list_edit(id, "lock", &format!("slot{slot}"), on);
    }

    /// Set (or with `None` remove) set `id`'s explicit shader (M6 spec §7).
    pub fn set_shader(&mut self, id: &str, shader: Option<engine::shader::Shader>) {
        if self.read_only() {
            return;
        }
        // Clearing a set with no table changes nothing (no table, no dirty, no undo step).
        let lower = fold_id(id);
        let has_table = self
            .f
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
        self.f.touch();
    }

    /// F3b D2: set (or with `None` remove) the base name of set `id`'s outputs.
    pub fn set_output_name(&mut self, id: &str, name: Option<&str>) {
        if self.read_only() {
            return;
        }
        let lower = fold_id(id);
        let has_table = self
            .f
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
        if name.is_none() && !has_table {
            return;
        }
        let t = self.table_mut(id);
        match name {
            Some(n) => {
                t.insert("output_name", toml_edit::value(n));
            }
            None => {
                t.remove("output_name");
            }
        }
        self.f.touch();
    }

    /// Set (or with `None` remove) JSON setting `key` (a dotted schema path) of set `id`. On a set with a texture-set
    /// block a mapped key lands in the block (design §5.1: one source, so the two files cannot drift).
    pub fn set_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        if t.get(Kind::Txst.key()).is_some_and(Item::is_table_like) {
            if let Some((cs, cv)) = cs_pbr::pgp_to_cs(key, v.as_ref()) {
                write_path(t, key, None);
                if let Some(b) = sub_table(t, Kind::Txst.key(), || Item::Table(Table::new())) {
                    match &cv {
                        Some(x) => write_json(b, cs, x),
                        None => write_path(b, cs, None),
                    };
                }
                self.f.touch();
                return;
            }
        }
        if write_path(t, key, v) {
            self.f.touch();
        }
    }

    /// Design §3: set (or with `None` remove) key `key` (dotted, `glintParameters.enabled`) of set `id`'s `kind`
    /// block, creating the block (`[sets.cs_txst]`) when missing.
    pub fn set_cs_value(&mut self, id: &str, kind: Kind, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        // A new block takes over the set's mapped flat keys (and marks the document dirty), as `add_cs_block` does.
        self.add_cs_block(id, kind);
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        let hit = match &v {
            Some(v) => write_json(b, key, v),
            None => write_path(b, key, None),
        };
        if hit {
            self.f.touch();
        }
    }

    /// Complex material design §3: set (or with `None` remove) key `key` of set `id`'s `[sets.cm]`, creating the block
    /// when missing. Keys are written as given; `cm_json::block_of` decides what is used.
    pub fn set_cm_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        // Removing from a set with no block changes nothing (no table, no dirty, no undo step).
        if v.is_none()
            && !self
                .parse_entries()
                .iter()
                .any(|e| same_id(&e.id, id) && e.cm.is_some())
        {
            return;
        }
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, "cm", || Item::Table(Table::new())) else {
            return;
        };
        if write_path(b, key, v) {
            self.f.touch();
        }
    }

    /// Tick or untick the export (D3). Unticking writes `export = false` and keeps the values, so ticking again
    /// restores them.
    pub fn set_cm_export(&mut self, id: &str, on: bool) {
        self.set_cm_value(id, "export", Some(Json::Bool(on)));
    }

    /// Design §3: start set `id`'s `kind` block (`edids = []`); nothing when it exists. A texture-set block takes
    /// over the set's own values of the keys it maps (moved, so adding the block never drops them, design §5.1).
    pub fn add_cs_block(&mut self, id: &str, kind: Kind) {
        if self.read_only() {
            return;
        }
        let own = self
            .parse_entries()
            .into_iter()
            .find(|e| same_id(&e.id, id))
            .map(|e| e.settings)
            .unwrap_or_default();
        let t = self.table_mut(id);
        if t.contains_key(kind.key()) {
            return;
        }
        let Some(b) = sub_table(t, kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        set_value(b, "edids", Value::Array(Array::new()));
        if kind == Kind::Txst {
            for (_, pgp) in cs_pbr::PGP_MAP {
                if let Some((cs, Some(v))) = cs_pbr::pgp_to_cs(pgp, own.get(pgp)) {
                    write_json(b, cs, &v);
                }
            }
            for (_, pgp) in cs_pbr::PGP_MAP {
                write_path(t, pgp, None);
            }
        }
        self.f.touch();
    }

    pub fn remove_cs_block(&mut self, id: &str, kind: Kind) {
        if self.read_only() {
            return;
        }
        // Mirrors `add_cs_block`: the PGPatcher values the block took over move back to the set (design §5.1).
        let back = (kind == Kind::Txst)
            .then(|| {
                self.parse_entries()
                    .into_iter()
                    .find(|e| same_id(&e.id, id))
            })
            .flatten()
            .and_then(|e| e.cs_txst)
            .map(|b| cs_pbr::cs_to_pgp(&b.values))
            .unwrap_or_default();
        let t = self.table_mut(id);
        if t.remove(kind.key()).is_some() {
            for (k, v) in back {
                write_path(t, &k, Some(v));
            }
            self.f.touch();
        }
    }

    /// Design §4: the template's values replace the block's known fields (unknown keys stay) and it is recorded as
    /// `template`; a block with no EDIDs takes the template's (plan ruling R8).
    pub fn apply_cs_template(&mut self, id: &str, t: &Template) {
        if self.read_only() {
            return;
        }
        self.add_cs_block(id, t.kind);
        let tbl = self.table_mut(id);
        let Some(b) = sub_table(tbl, t.kind.key(), || Item::Table(Table::new())) else {
            return;
        };
        for f in fields(t.kind) {
            b.remove(f.name);
        }
        for (k, v) in &t.values {
            if field(t.kind, k).is_some() {
                write_json(b, k, v);
            }
        }
        set_value(b, "template", Value::from(t.name.as_str()));
        if strings(b.get("edids")).is_empty() {
            let mut arr = Array::new();
            for e in &t.edids {
                arr.push(e.as_str());
            }
            set_value(b, "edids", Value::Array(arr));
        }
        self.f.touch();
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
                // Pruning an emptied (or already empty) sub-table is a change too: the caller touches.
                let pruned = sub.is_empty();
                if pruned {
                    t.remove(parts[0]);
                }
                hit || pruned
            }
            remove(t, &parts)
        }
    }
}

/// `write_path` for a value that may be a JSON object (a block's `glintParameters`): its keys go one level down.
fn write_json(t: &mut dyn TableLike, path: &str, v: &Json) -> bool {
    match v {
        Json::Object(o) => o.iter().fold(false, |hit, (k, x)| {
            write_json(t, &format!("{path}.{k}"), x) | hit
        }),
        _ => write_path(t, path, Some(v.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// REPORT #9: after every mutator the cached `entries()` is what the document holds.
    #[test]
    fn the_entries_cache_follows_every_mutator() {
        let d = tempfile::tempdir().unwrap();
        let mut s = SetsDoc::open(d.path());
        let template = Template {
            name: "t".into(),
            kind: Kind::Txst,
            edids: vec!["E".into()],
            values: serde_json::Map::new(),
        };
        type Step<'a> = (&'a str, Box<dyn Fn(&mut SetsDoc) + 'a>);
        let steps: Vec<Step> = vec![
            ("create_set", Box::new(|s| s.create_set("a/x"))),
            ("add_member", Box::new(|s| s.add_member("a/x", "a/f.png"))),
            (
                "remove_member",
                Box::new(|s| s.remove_member("a/x", "a/g.png")),
            ),
            (
                "set_borrow",
                Box::new(|s| s.set_borrow("a/x", 2, Some("b/y"))),
            ),
            ("set_lock", Box::new(|s| s.set_lock("a/x", 2, true))),
            (
                "set_shader",
                Box::new(|s| s.set_shader("a/x", Some(engine::shader::Shader::Pbr))),
            ),
            (
                "set_output_name",
                Box::new(|s| s.set_output_name("a/x", Some("steel"))),
            ),
            (
                "set_value",
                Box::new(|s| s.set_value("a/x", "hair", Some(json!(true)))),
            ),
            (
                "set_value on a new set",
                Box::new(|s| s.set_value("c/z", "hair", None)),
            ),
            (
                "add_cs_block",
                Box::new(|s| s.add_cs_block("a/x", Kind::Txst)),
            ),
            (
                "set_cs_value",
                Box::new(|s| s.set_cs_value("a/x", Kind::Mato, "edids", Some(json!(["M"])))),
            ),
            (
                "apply_cs_template",
                Box::new(|s| s.apply_cs_template("a/x", &template)),
            ),
            (
                "remove_cs_block",
                Box::new(|s| s.remove_cs_block("a/x", Kind::Txst)),
            ),
            (
                "remove_cs_block on a new set",
                Box::new(|s| s.remove_cs_block("d/w", Kind::Txst)),
            ),
            (
                "set_cm_value",
                Box::new(|s| s.set_cm_value("a/x", "glossiness", Some(json!(0.5)))),
            ),
            ("set_cm_export", Box::new(|s| s.set_cm_export("a/x", false))),
            (
                "set_order",
                Box::new(|s| s.set_order(&["a/x".into(), "b/y".into()])),
            ),
            ("rename_set", Box::new(|s| s.rename_set("a/x", "a/renamed"))),
            (
                "rename_set (no order)",
                Box::new(|s| {
                    s.set_order(&[]);
                    s.rename_set("a/renamed", "a/again")
                }),
            ),
            ("retarget_auto", Box::new(|s| s.retarget_auto("a/x", "a/y"))),
            ("delete_set", Box::new(|s| s.delete_set("c/z"))),
            ("add_section", Box::new(|s| s.add_section(&Settings::new()))),
            ("set_json_name", Box::new(|s| s.set_json_name(Some("n")))),
            (
                "restore_keeping_json_sha",
                Box::new(|s| s.restore_keeping_json_sha("[[sets]]\nid = \"r/one\"\n")),
            ),
            (
                "restore",
                Box::new(|s| s.restore("[[sets]]\nid = \"r/two\"\n")),
            ),
            ("reload", Box::new(|s| s.reload())),
        ];
        for (what, step) in &steps {
            s.entries();
            step(&mut s);
            assert_eq!(*s.entries(), s.parse_entries(), "{what}");
        }
    }

    /// Review M1: removing the last key of a block, or a key under an empty sub-table that gets pruned, is a change:
    /// rev moves, the document is dirty and the cached entries match.
    #[test]
    fn pruning_a_block_sub_table_bumps_the_revision() {
        let d = tempfile::tempdir().unwrap();
        let mut s = SetsDoc::open(d.path());
        s.restore(
            "[[sets]]\nid = \"a/x\"\n[sets.cm]\nglossiness = 0.5\n[sets.cm.extra]\n\
             [sets.cs_txst]\nedids = []\n[sets.cs_txst.glintParameters]\n",
        );
        type Step<'a> = (&'a str, Box<dyn Fn(&mut SetsDoc) + 'a>);
        let steps: Vec<Step> = vec![
            (
                "cm: empty sub-table pruned",
                Box::new(|s| s.set_cm_value("a/x", "extra.k", None)),
            ),
            (
                "cm: last key",
                Box::new(|s| s.set_cm_value("a/x", "glossiness", None)),
            ),
            (
                "cs: empty sub-table pruned",
                Box::new(|s| s.set_cs_value("a/x", Kind::Txst, "glintParameters.enabled", None)),
            ),
        ];
        for (what, step) in &steps {
            s.entries();
            let rev = s.rev();
            let text = s.text();
            step(&mut s);
            assert_ne!(s.text(), text, "{what}: the document changed");
            assert_ne!(s.rev(), rev, "{what}");
            assert!(s.is_dirty(), "{what}");
            assert_eq!(*s.entries(), s.parse_entries(), "{what}");
        }
    }
}
