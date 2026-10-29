//! M6 Track F3b (spec §4, §5, §7): the rename dialog's model. Inputs mode gives files an `[inputs]` alias in
//! `sdc.toml` (the file on disk is never touched); Outputs mode gives sets an output base name (`output_name`, or
//! the PGPatcher `rename` on a CS PBR set). One pure plan, `plan_rename`, feeds both the preview and Apply.
use super::items::ItemKind;
use super::sets::{check_output_name, renamed_output, slot_of, views, SetView};
use super::{AppModel, Dialog};
use crate::pgp::prefix::check_file_name;
use engine::config::Target;
use engine::discover::{canonicalise_outputs, InputFile, InputKind};
use engine::group::{split_suffix, split_vocab};
use engine::sets::fold_id;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenameMode {
    /// `[inputs]` aliases: the files are read under the new names.
    #[default]
    Inputs,
    /// Output base names per set.
    Outputs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Case {
    #[default]
    Keep,
    Lower,
    Upper,
}

/// What the dialog does to each base name, in this order: find and replace, prefix, suffix, case (D4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recipe {
    pub find: String,
    pub replace: String,
    /// `find` is a regular expression (the `regex` crate: linear time, `$1` and `${name}` in `replace`).
    pub regex: bool,
    pub prefix: String,
    pub suffix: String,
    pub case: Case,
    /// Inputs: the recipe runs over the folders and the base name, not the base name alone (the map suffix and the
    /// extension still stay).
    pub path: bool,
}

impl Recipe {
    fn compile(&self) -> Result<Option<Regex>, String> {
        if !self.regex || self.find.is_empty() {
            return Ok(None);
        }
        Regex::new(&self.find)
            .map(Some)
            .map_err(|e| format!("pattern: {e}"))
    }

    /// `base` after the recipe, and whether Find matched it (always true with an empty Find).
    fn apply(&self, re: Option<&Regex>, base: &str) -> (String, bool) {
        let (replaced, matched) = match re {
            Some(re) => (
                re.replace_all(base, self.replace.as_str()).into_owned(),
                re.is_match(base),
            ),
            None if self.find.is_empty() => (base.to_string(), true),
            None => (
                base.replace(&self.find, &self.replace),
                base.contains(&self.find),
            ),
        };
        let joined = format!("{}{replaced}{}", self.prefix, self.suffix);
        let out = match self.case {
            Case::Keep => joined,
            Case::Lower => joined.to_lowercase(),
            Case::Upper => joined.to_uppercase(),
        };
        (out, matched)
    }
}

/// The open dialog: what it renames and how.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameDialog {
    pub mode: RenameMode,
    /// Item keys (Inputs) or set ids (Outputs), in the selection's order.
    pub keys: Vec<String>,
    pub recipe: Recipe,
    /// Rows left out of Apply.
    pub exclude: BTreeSet<String>,
    /// Rows typed by hand: a whole project-relative path (Inputs) or a whole base name (Outputs).
    pub typed: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameRow {
    /// The item key (Inputs) or the set id (Outputs).
    pub key: String,
    /// Inputs: the path the file is read as now. Outputs: the base name the set's outputs have now.
    pub old: String,
    pub new: String,
    /// Inputs: the name's map suffix before and after ("colour" for none), so a newly read `_n` shows.
    pub kind: Option<(String, String)>,
    /// The output after Apply (Outputs: the first one, with how many more).
    pub output: String,
    pub included: bool,
    /// Find matched this row's base name.
    pub matched: bool,
    /// Why this row cannot be applied; any on an included row blocks Apply.
    pub problem: Option<String>,
    pub warning: Option<String>,
    /// Why this row is never written ("skipped (locked)").
    pub skipped: Option<String>,
}

impl RenameRow {
    /// Apply writes this row.
    pub fn writes(&self) -> bool {
        self.included && self.skipped.is_none() && self.problem.is_none() && self.new != self.old
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenamePlan {
    pub rows: Vec<RenameRow>,
    /// The recipe itself is wrong (an invalid pattern); nothing applies.
    pub error: Option<String>,
}

impl RenamePlan {
    /// "N of M matched", over the rows that can change.
    pub fn matched(&self) -> (usize, usize) {
        let live = self.rows.iter().filter(|r| r.skipped.is_none());
        let (mut n, mut m) = (0, 0);
        for r in live {
            m += 1;
            n += usize::from(r.matched);
        }
        (n, m)
    }

    /// Apply is enabled: no recipe error, no problem on an included row, and something to write.
    pub fn can_apply(&self) -> bool {
        self.error.is_none()
            && self
                .rows
                .iter()
                .all(|r| !r.included || r.skipped.is_some() || r.problem.is_none())
            && self.rows.iter().any(RenameRow::writes)
    }
}

/// The plan the dialog last drew, with what it was made from (`AppModel::rename_plan_cached`).
pub(crate) struct PlanCache {
    key: (super::ViewKey, u64),
    dialog: RenameDialog,
    plan: Arc<RenamePlan>,
}

const PACKED: &str = "skipped (a packed output has no file of its own)";
const LOCKED: &str = "skipped (locked)";

/// The part of a file stem before its map suffix or exporter term (`wall_normal` → `wall`).
pub(crate) fn base_of(stem: &str) -> &str {
    let base = split_vocab(stem).map_or_else(|| split_suffix(stem).0, |(b, _)| b);
    // Both splits cut a tail off `stem`, so the base is a prefix of it.
    &stem[..base.len().min(stem.len())]
}

/// The tail a name ends in, as the kind column shows it: `_n`, `_normal`, or "colour".
fn role_of(stem: &str) -> String {
    match &stem[base_of(stem).len()..] {
        "" => "colour".into(),
        t => t.to_string(),
    }
}

pub(crate) fn split_path(p: &str) -> (&str, &str) {
    match p.rfind('/') {
        Some(i) => (&p[..i + 1], &p[i + 1..]),
        None => ("", p),
    }
}

pub(crate) fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

fn lower(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

/// The output path the engine gives a file read as `rel` (Skyrim suffix names applied).
pub(crate) fn input_output(rel: &str) -> String {
    let mut f = [InputFile {
        path: PathBuf::from(rel),
        rel: PathBuf::from(rel),
        game_path: String::new(),
        output: PathBuf::from(rel).with_extension("dds"),
        kind: InputKind::Image,
        real_rel: None,
    }];
    canonicalise_outputs(&mut f, &BTreeMap::new());
    f[0].output.to_string_lossy().replace('\\', "/")
}

thread_local! {
    // Per thread: tests run side by side, each drawing on its own thread.
    static PLANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How often this thread ran `plan_rename` (tests: the dialog plans once per change, not once per frame).
pub fn plan_calls() -> usize {
    PLANS.get()
}

/// F3b spec §4: every row of the dialog for `d`, with its checks. Pure: reads the model, writes nothing.
pub fn plan_rename(m: &AppModel, d: &RenameDialog) -> RenamePlan {
    PLANS.set(PLANS.get() + 1);
    let re = match d.recipe.compile() {
        Ok(re) => re,
        Err(e) => {
            return RenamePlan {
                rows: vec![],
                error: Some(e),
            }
        }
    };
    let mut rows = match d.mode {
        RenameMode::Inputs => input_rows(m, d, re.as_ref()),
        RenameMode::Outputs => output_rows(m, d, re.as_ref()),
    };
    for r in &mut rows {
        r.included = !d.exclude.contains(&r.key);
    }
    match d.mode {
        RenameMode::Inputs => input_collisions(m, &mut rows),
        RenameMode::Outputs => output_collisions(m, &mut rows),
    }
    RenamePlan { rows, error: None }
}

fn row(key: &str, old: String) -> RenameRow {
    RenameRow {
        key: key.to_string(),
        new: old.clone(),
        old,
        kind: None,
        output: String::new(),
        included: true,
        matched: false,
        problem: None,
        warning: None,
        skipped: None,
    }
}

/// Per member, the slots some set holding it has locked; built once per plan (Note FA).
fn locked_slots(views: &[SetView]) -> HashMap<&str, BTreeSet<u8>> {
    let mut out: HashMap<&str, BTreeSet<u8>> = HashMap::new();
    for v in views.iter().filter(|v| !v.locks.is_empty()) {
        for k in &v.members {
            out.entry(k).or_default().extend(&v.locks);
        }
    }
    out
}

/// The slot `item` fills in its set is locked (F3a Task 8).
fn fills_locked_slot(locked: &HashMap<&str, BTreeSet<u8>>, m: &AppModel, key: &str) -> bool {
    let Some(it) = m.item(key) else { return false };
    let Some(slot) = slot_of(it) else {
        return false;
    };
    locked
        .get(key)
        .is_some_and(|l| l.contains(&slot.slot_number()))
}

/// The index of the first set per folded id (`same_id`'s fold); the first one wins, as a front-to-back search did.
fn first_by_id(views: &[SetView]) -> HashMap<String, usize> {
    let mut out = HashMap::with_capacity(views.len());
    for (i, v) in views.iter().enumerate() {
        out.entry(fold_id(&v.id)).or_insert(i);
    }
    out
}

fn input_rows(m: &AppModel, d: &RenameDialog, re: Option<&Regex>) -> Vec<RenameRow> {
    let views = views(m);
    let locked = locked_slots(&views);
    let project = m.project.as_ref();
    let mut out = vec![];
    for key in &d.keys {
        let Some(it) = m.item(key) else { continue };
        let current = project
            .and_then(|p| p.read_as(key))
            .map_or_else(|| key.clone(), |a| a.replace('\\', "/"));
        let mut r = row(key, current.clone());
        if it.kind == ItemKind::Packed {
            r.skipped = Some(PACKED.into());
            out.push(r);
            continue;
        }
        if fills_locked_slot(&locked, m, key) {
            r.skipped = Some(LOCKED.into());
        }
        let real_ext = split_ext(split_path(key).1).1.to_string();
        let (dir, name) = split_path(&current);
        let stem = split_ext(name).0;
        let base = base_of(stem);
        let tail = &stem[base.len()..];
        let new = match d.typed.get(key) {
            Some(t) => {
                let t = t.trim().replace('\\', "/");
                let t = t.trim_start_matches("./").to_string();
                let (_, tname) = split_path(&t);
                if split_ext(tname).1.is_empty() && !real_ext.is_empty() {
                    format!("{t}.{real_ext}")
                } else {
                    t
                }
            }
            None if d.recipe.path => {
                let (p, matched) = d.recipe.apply(re, &format!("{dir}{base}"));
                r.matched = matched;
                format!("{p}{tail}.{real_ext}")
            }
            None => {
                let (b, matched) = d.recipe.apply(re, base);
                r.matched = matched;
                format!("{dir}{b}{tail}.{real_ext}")
            }
        };
        if d.typed.contains_key(key) {
            r.matched = true;
        }
        let new_stem = split_ext(split_path(&new).1).0.to_string();
        r.kind = Some((role_of(stem), role_of(&new_stem)));
        r.output = input_output(&new);
        r.problem = input_problem(&new, &real_ext);
        if r.problem.is_none() && r.kind.as_ref().is_some_and(|(a, b)| a != b) {
            let (a, b) = r.kind.clone().unwrap_or_default();
            r.warning = Some(format!("read as {b} now (was {a})"));
        }
        r.new = new;
        out.push(r);
    }
    out
}

fn input_problem(new: &str, real_ext: &str) -> Option<String> {
    if new.starts_with('/') || new.as_bytes().get(1) == Some(&b':') {
        return Some("a path relative to the folder holding sdc.toml".into());
    }
    for part in new.split('/') {
        if part == ".." {
            return Some("cannot leave the folder holding sdc.toml (..)".into());
        }
        if let Err(e) = check_file_name(part, "name") {
            return Some(e.0);
        }
    }
    let ext = split_ext(split_path(new).1).1;
    if !ext.eq_ignore_ascii_case(real_ext) {
        return Some(format!(
            "keeps the file's own extension (.{real_ext}, not .{ext})"
        ));
    }
    if split_ext(split_path(new).1).0.is_empty() {
        return Some("names no file".into());
    }
    None
}

/// Two files read under one name, or a name another file has on disk, block (the engine would refuse the alias).
fn input_collisions(m: &AppModel, rows: &mut [RenameRow]) {
    let changing: BTreeSet<String> = rows
        .iter()
        .filter(|r| r.writes())
        .map(|r| r.key.clone())
        .collect();
    // Real paths of every file, and the current name of every file this batch leaves alone.
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    for it in m.items.iter().filter(|i| i.kind != ItemKind::Packed) {
        taken.insert(lower(&it.key), it.key.clone());
        if !changing.contains(&it.key) {
            if let Some(a) = m.project.as_ref().and_then(|p| p.read_as(&it.key)) {
                taken.insert(lower(&a), it.key.clone());
            }
        }
    }
    // #19: outputs of every file this batch leaves alone (an `output_name` can point one anywhere).
    let writes: BTreeMap<String, String> = m
        .items
        .iter()
        .filter(|i| !i.job_key.is_empty() && !changing.contains(&i.key))
        .map(|i| (lower(&i.job_key.replace('\\', "/")), i.key.clone()))
        .collect();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut seen_out: BTreeMap<String, String> = BTreeMap::new();
    for r in rows.iter_mut().filter(|r| r.writes()) {
        let l = lower(&r.new);
        if let Some(other) = taken.get(&l).filter(|o| **o != r.key) {
            r.problem = Some(format!("{other} already has this name"));
        } else if let Some(other) = writes.get(&lower(&r.output)) {
            r.problem = Some(format!("{other} already writes {}", r.output));
        } else if let Some(other) = seen.get(&l) {
            r.problem = Some(format!("{other} is renamed to this name too"));
        } else if let Some(other) = seen_out.get(&lower(&r.output)) {
            // Different names (a.png, b.tga), one output (x.dds).
            r.problem = Some(format!("{other} is renamed to write {} too", r.output));
        } else {
            seen.insert(l, r.key.clone());
            seen_out.insert(lower(&r.output), r.key.clone());
        }
    }
}

fn output_rows(m: &AppModel, d: &RenameDialog, re: Option<&Regex>) -> Vec<RenameRow> {
    let views = views(m);
    let index = first_by_id(&views);
    let mut out = vec![];
    for id in &d.keys {
        let Some(v) = index.get(&fold_id(id)).map(|&i| &views[i]) else {
            continue;
        };
        let outputs = set_outputs(m, v);
        let first = outputs.first().map(|(_, jk)| jk.clone());
        let Some(first) = first else {
            let mut r = row(&v.id, String::new());
            r.skipped = Some("skipped (the set writes no output)".into());
            out.push(r);
            continue;
        };
        let stem = split_ext(split_path(&first).1).0.to_string();
        let old = base_of(&stem).to_string();
        let mut r = row(&v.id, old.clone());
        let new = match d.typed.get(&v.id) {
            Some(t) => {
                r.matched = true;
                t.trim().to_string()
            }
            None => {
                let (n, matched) = d.recipe.apply(re, &old);
                r.matched = matched;
                n
            }
        };
        r.problem = check_output_name(&new).err();
        let renamed: Vec<String> = outputs
            .iter()
            .filter_map(|(_, jk)| renamed_output(jk, &new))
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        r.output = match renamed.len() {
            0 => String::new(),
            1 => renamed[0].clone(),
            n => format!("{} (+{} more)", renamed[0], n - 1),
        };
        let pbr = v.pbr && m.target == Target::Cs;
        if !pbr && new != old {
            let meshes: usize = outputs.iter().map(|(k, _)| m.meshes_using(k)).sum();
            if meshes > 0 {
                r.warning = Some(format!(
                    "{meshes} mesh{} still use the old name",
                    if meshes == 1 { "" } else { "es" }
                ));
            }
        }
        r.new = new;
        out.push(r);
    }
    out
}

/// (item key, job key) of every output set `v` writes, diffuse first.
fn set_outputs(m: &AppModel, v: &SetView) -> Vec<(String, String)> {
    let mut outs: Vec<(String, String)> = v
        .members
        .iter()
        .filter_map(|k| m.item(k))
        .filter(|i| i.job.is_some() && !i.job_key.is_empty())
        .map(|i| (i.key.clone(), i.job_key.replace('\\', "/")))
        .collect();
    outs.sort_by_key(|(k, _)| {
        m.item(k)
            .and_then(slot_of)
            .is_none_or(|s| s != super::sets::SlotKind::Diffuse)
    });
    outs
}

/// An output another texture writes, or two sets renamed onto one output, block.
fn output_collisions(m: &AppModel, rows: &mut [RenameRow]) {
    let views = views(m);
    let index = first_by_id(&views);
    // Each set's outputs, once (the two loops below both read them).
    let outs: Vec<Vec<(String, String)>> = views.iter().map(|v| set_outputs(m, v)).collect();
    let changing: HashSet<String> = rows
        .iter()
        .filter(|r| r.writes())
        .map(|r| fold_id(&r.key))
        .collect();
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    for (v, outs) in views.iter().zip(&outs) {
        if changing.contains(&fold_id(&v.id)) {
            continue;
        }
        for (_, jk) in outs {
            taken.insert(lower(jk), v.id.clone());
        }
    }
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for r in rows.iter_mut().filter(|r| r.writes()) {
        let Some(&at) = index.get(&fold_id(&r.key)) else {
            continue;
        };
        for (_, jk) in &outs[at] {
            let Some(p) = renamed_output(jk, &r.new) else {
                continue;
            };
            let l = lower(&p.to_string_lossy());
            if let Some(other) = taken.get(&l) {
                r.problem = Some(format!("{other} already writes {}", p.display()));
                break;
            }
            if let Some(other) = seen.get(&l).filter(|o| **o != r.key) {
                r.problem = Some(format!("{other} is renamed onto {} too", p.display()));
                break;
            }
            seen.insert(l, r.key.clone());
        }
    }
}

impl AppModel {
    /// F3b D2: how many scanned mesh shapes use texture `item`'s current output in slot 0 (the scan's mesh evidence,
    /// by `shader::colour_mesh_key`). 0 for a map meshes do not name there, or with no scan.
    pub fn meshes_using(&self, item: &str) -> usize {
        let Some(j) = self.item(item).and_then(|i| i.job.as_ref()) else {
            return 0;
        };
        if j.pack.is_some() {
            return 0;
        }
        engine::shader::colour_mesh_key(&j.game_path, &j.output, Some(j.profile.subtype))
            .and_then(|k| self.mesh_alpha.get(&k))
            .map_or(0, Vec::len)
    }

    /// The textures or sets a rename in `mode` covers, from the selection (spec §6): Inputs takes the selected
    /// textures, or every member of the selected sets; Outputs takes the selected sets, or the sets of the selected
    /// textures.
    pub fn rename_keys(&self, mode: RenameMode) -> Vec<String> {
        let views = views(self);
        // First match wins, as a front-to-back search over `views` did.
        let mut by_key: HashMap<&str, &SetView> = HashMap::new();
        let selected = self.selected_sets();
        if !selected.is_empty() {
            for v in views.iter() {
                by_key.entry(&v.key).or_insert(v);
            }
        }
        let sets: Vec<&SetView> = selected
            .iter()
            .filter_map(|k| by_key.get(k.as_str()).copied())
            .collect();
        let items = self.selected_items();
        // `out` is the answer and keeps the order; `seen` only answers "already in `out`?".
        let mut out: Vec<String> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        let mut push = |k: &str| {
            if seen.insert(k.to_string()) {
                out.push(k.to_string());
            }
        };
        match mode {
            RenameMode::Inputs if sets.is_empty() => items.iter().for_each(|k| push(k)),
            RenameMode::Inputs => sets.iter().flat_map(|v| &v.members).for_each(|k| push(k)),
            RenameMode::Outputs if sets.is_empty() => {
                let mut set_of: HashMap<&str, &str> = HashMap::new();
                for v in views.iter() {
                    for k in &v.members {
                        set_of.entry(k).or_insert(&v.id);
                    }
                }
                for k in &items {
                    if let Some(id) = set_of.get(k.as_str()) {
                        push(id);
                    }
                }
            }
            RenameMode::Outputs => sets.iter().for_each(|v| push(&v.id)),
        }
        out
    }

    /// Open the rename dialog in `mode` over the selection, with the last recipe of this session.
    pub fn open_rename(&mut self, mode: RenameMode) {
        let keys = self.rename_keys(mode);
        if keys.is_empty() {
            return;
        }
        self.dialog = Some(Dialog::Rename(RenameDialog {
            mode,
            keys,
            recipe: self.last_recipe.clone(),
            ..RenameDialog::default()
        }));
    }

    /// `plan_rename(self, d)` for drawing: the last plan again while the dialog and everything the plan reads are as
    /// they were (the key `views()` uses, which covers the items, the sets, the target and the scan, and the project
    /// document's revision for the `[inputs]` aliases). Apply never uses it: `apply_rename` plans for itself.
    pub fn rename_plan_cached(&self, d: &RenameDialog) -> Arc<RenamePlan> {
        let key = (
            self.view_key(),
            self.project.as_ref().map_or(0, |p| p.rev()),
        );
        if let Some(c) = self.rename_cache.borrow().as_ref() {
            if c.key == key && c.dialog == *d {
                return c.plan.clone();
            }
        }
        let plan = Arc::new(plan_rename(self, d));
        *self.rename_cache.borrow_mut() = Some(PlanCache {
            key,
            dialog: d.clone(),
            plan: plan.clone(),
        });
        plan
    }

    /// Drop the cached plan (the dialog closed); nothing reads a stale one, the key is checked on every use.
    pub fn clear_rename_cache(&self) {
        self.rename_cache.take();
    }
    /// The open rename dialog's plan.
    pub fn rename_plan(&self) -> Option<RenamePlan> {
        match &self.dialog {
            Some(Dialog::Rename(d)) => Some(plan_rename(self, d)),
            _ => None,
        }
    }

    /// Switch the open dialog's mode; the recipe stays, the keys follow the selection.
    pub fn set_rename_mode(&mut self, mode: RenameMode) {
        let keys = self.rename_keys(mode);
        if let Some(Dialog::Rename(d)) = self.dialog.as_mut() {
            if d.mode != mode && !keys.is_empty() {
                d.mode = mode;
                d.keys = keys;
                d.exclude.clear();
                d.typed.clear();
            }
        }
    }

    /// Apply the open dialog (spec §7): every writable row in one undo step and one plan. Nothing happens while
    /// Apply would be disabled. True when it applied.
    pub fn apply_rename(&mut self) -> bool {
        let Some(Dialog::Rename(d)) = self.dialog.clone() else {
            return false;
        };
        // Review I1: the aliases cannot be written, so nothing (not even the set entries) may move.
        if d.mode == RenameMode::Inputs
            && (self.project.as_ref().is_some_and(|p| p.read_only())
                || self.sets_doc.as_ref().is_some_and(|s| s.read_only()))
        {
            return false;
        }
        let plan = plan_rename(self, &d);
        if !plan.can_apply() {
            return false;
        }
        self.last_recipe = d.recipe.clone();
        let rows: Vec<RenameRow> = plan.rows.into_iter().filter(RenameRow::writes).collect();
        let n = rows.len();
        let label = match d.mode {
            RenameMode::Inputs => format!("Rename {n} input{}", if n == 1 { "" } else { "s" }),
            RenameMode::Outputs => {
                format!("Rename outputs of {n} set{}", if n == 1 { "" } else { "s" })
            }
        };
        // AB1: each set whose own diffuse is renamed takes its sdc-project.toml entry to its new automatic id.
        let retargets: Vec<(String, String)> = match d.mode {
            RenameMode::Inputs => super::sets::set_views(self)
                .iter()
                .filter_map(|v| {
                    let Some(super::sets::SlotFill::Own { item }) =
                        v.slots.get(&super::sets::SlotKind::Diffuse)
                    else {
                        return None;
                    };
                    let r = rows.iter().find(|r| &r.key == item)?;
                    self.auto_retarget(&v.key, item, &r.new)
                })
                .collect(),
            RenameMode::Outputs => vec![],
        };
        self.dialog = None;
        self.clear_rename_cache();
        self.note_kept_entries(&retargets);
        self.batch_edit(&label, |m| {
            for (from, to) in &retargets {
                m.edit_docs(&label, None, true, |_, d| d.retarget_auto(from, to));
            }
            for r in &rows {
                match d.mode {
                    RenameMode::Inputs => {
                        // Back to its own name: the alias goes.
                        let alias = (r.new != r.key).then_some(r.new.as_str());
                        m.edit_docs(&label, None, true, |p, _| p.set_read_as(&r.key, alias));
                    }
                    RenameMode::Outputs => {
                        // Refused only on a read-only sdc-project.toml, which the batch then leaves as it is.
                        let _ = m.set_output_name(&r.key, Some(&r.new));
                    }
                }
            }
        })
    }

    /// The selected textures that have an `[inputs]` alias.
    pub fn aliased_selection(&self) -> Vec<String> {
        let Some(p) = self.project.as_ref() else {
            return vec![];
        };
        self.rename_keys(RenameMode::Inputs)
            .into_iter()
            .filter(|k| p.read_as(k).is_some())
            .collect()
    }

    /// Remove the `[inputs]` aliases of `keys`: one undo step and one plan.
    pub fn clear_input_alias(&mut self, keys: &[String]) -> bool {
        let n = keys.len();
        self.batch_edit(
            &format!("Clear {n} input alias{}", if n == 1 { "" } else { "es" }),
            |m| {
                for k in keys {
                    m.edit_docs("Clear input alias", None, true, |p, _| {
                        p.set_read_as(k, None)
                    });
                }
            },
        )
    }

    /// The name texture `key` is read as, when an `[inputs]` alias renames it.
    pub fn read_as_of(&self, key: &str) -> Option<String> {
        self.project.as_ref()?.read_as(key)
    }
}
