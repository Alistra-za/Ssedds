//! The preset creator (Note AM, owner design §2). User presets live in the presets folder
//! (`AppModel::user_presets`: `%APPDATA%\sdc\presets` in the app, `None` or a temporary folder in tests). A preset is
//! edited as its own TOML table, so whatever the creator does not show (defaults, other rules, a description) is
//! kept, and it is loaded through the engine's preset loader (`PresetRegistry::add_toml` + `check_preset`, the path
//! the presets folder takes) before any file is written.
use engine::config::{Backend, Category, Format, Profile, Subtype, Target};
use engine::presets::{finish_profile, merge_json, PresetFile, PresetRegistry};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The fill roles the creator edits (owner design §2).
pub const FILL_ROLES: [&str; 5] = ["roughness", "metallic", "ao", "specular", "height"];

/// The sources a swizzle channel can take (`channels.swizzle`).
pub const SWIZZLE_SOURCES: [char; 6] = ['r', 'g', 'b', 'a', '0', '1'];

/// The read-only ORM example (Note AL): engine behaviour, never written to a preset.
pub const ORM_EXAMPLE: &str =
    "ORM → RMAOS (built in): an _orm file gives the _rmaos R = roughness from its G, \
G = metallic from its B, B = AO from its R; specular (A) comes from packing.fill.specular, else 1.";

/// The row groups (AM5), each in `Subtype::ALL` order.
pub const GROUPS: [&str; 4] = ["Colour", "Normal", "Data", "Special"];

pub fn group_of(st: Subtype) -> &'static str {
    if st == Subtype::ComplexGrass {
        return "Special";
    }
    match st.category() {
        Category::Color => "Colour",
        Category::Normal => "Normal",
        Category::Grayscale | Category::MixedPacked => "Data",
        Category::Hdr => "Special",
    }
}

/// Data sub-types get the Channel remap and Packing fills subheaders.
pub fn is_data(st: Subtype) -> bool {
    group_of(st) == "Data"
}

/// The canonical suffixes that can name `st` under `target`: what `engine::suggest::suggest` answers for each §6.1
/// suffix over the contexts that change its answer (a PBR, `_msn` or Advanced skin set, varying alpha, a landscape
/// or interface path).
pub fn suffixes_of(st: Subtype, target: Target) -> Vec<&'static str> {
    use engine::presets::AlphaContent;
    use engine::suggest::{suggest, SuggestContext};
    let sets: [BTreeSet<String>; 3] = [
        BTreeSet::new(),
        ["rmaos".to_string()].into(),
        ["msn".to_string()].into(),
    ];
    let paths = [
        "textures/a/x.png",
        "textures/landscape/x.png",
        "textures/interface/x.png",
    ];
    engine::group::SUFFIXES
        .into_iter()
        .filter(|s| {
            sets.iter().any(|set| {
                [false, true].into_iter().any(|skin| {
                    [AlphaContent::None, AlphaContent::Varying]
                        .into_iter()
                        .any(|alpha| {
                            paths.iter().any(|path| {
                                suggest(&SuggestContext {
                                    target,
                                    suffix: Some(s),
                                    game_path: path,
                                    set_suffixes: set,
                                    alpha,
                                    is_float: false,
                                    advanced_skin: skin,
                                })
                                .iter()
                                .any(|x| x.subtype == st)
                            })
                        })
                })
            })
        })
        .collect()
}

/// AM6: a preset id is lower-case letters, digits and '-', starting with a letter or digit (it names the file).
pub fn check_id(id: &str) -> Result<(), String> {
    let ok = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id.bytes().next().is_some_and(|b| b != b'-');
    if ok {
        Ok(())
    } else if id.is_empty() {
        Err("Give the preset a name".into())
    } else {
        Err(format!(
            "'{id}': a name is lower-case letters, digits and '-' (it becomes the file name)"
        ))
    }
}

/// The glob of an alias's swizzle remap rule: a glob without '/' matches the file name alone.
fn remap_glob(alias: &str) -> String {
    format!("*_{alias}.*")
}

/// One sub-type's row: the profile the draft resolves to and the base's, under the draft's target.
pub struct Row {
    pub subtype: Subtype,
    pub profile: Result<Profile, String>,
    pub base: Option<Profile>,
}

/// The preset being shown.
pub struct Draft {
    pub table: toml::Table,
    saved: toml::Table,
    /// The id of the loaded user preset this edits; `None` for a new preset or a duplicate.
    orig: Option<String>,
    /// A built-in, shown read-only (Duplicate makes an editable copy).
    pub read_only: bool,
}

pub struct PresetsModel {
    dir: Option<PathBuf>,
    registry: PresetRegistry,
    /// Files of the presets folder that did not load (CFG-PRESET-SKIPPED).
    pub skipped: Vec<String>,
    /// The target used when neither the preset nor its base names one.
    pub fallback: Target,
    pub draft: Option<Draft>,
    /// Recomputed after each change: the rows, and what loading the draft through the engine says.
    rows: Vec<Row>,
    status: Result<(), String>,
    /// The target the rows resolve under: the preset's, its base chain's, else `fallback`.
    pub target: Target,
    /// The last Save or Delete failure, until the next change.
    pub error: Option<String>,
}

fn json_of(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The table at `path` (created when `make`), or `None`.
fn table_at<'a>(t: &'a mut toml::Table, path: &[&str], make: bool) -> Option<&'a mut toml::Table> {
    let Some((first, rest)) = path.split_first() else {
        return Some(t);
    };
    if make && !t.contains_key(*first) {
        t.insert((*first).into(), toml::Value::Table(toml::Table::new()));
    }
    table_at(t.get_mut(*first)?.as_table_mut()?, rest, make)
}

/// Remove `path` and the tables it leaves empty.
fn remove_at(t: &mut toml::Table, path: &[&str]) {
    match path {
        [] => {}
        [k] => {
            t.remove(*k);
        }
        [k, rest @ ..] => {
            if let Some(sub) = t.get_mut(*k).and_then(toml::Value::as_table_mut) {
                remove_at(sub, rest);
                if sub.is_empty() {
                    t.remove(*k);
                }
            }
        }
    }
}

fn get_at<'a>(t: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Value> {
    let (last, parents) = path.split_last()?;
    let mut cur = t;
    for p in parents {
        cur = cur.get(*p)?.as_table()?;
    }
    cur.get(*last)
}

impl PresetsModel {
    pub fn new(dir: Option<PathBuf>, fallback: Target) -> PresetsModel {
        let mut m = PresetsModel {
            dir,
            registry: PresetRegistry::default(),
            skipped: vec![],
            fallback,
            draft: None,
            rows: vec![],
            status: Ok(()),
            target: fallback,
            error: None,
        };
        m.rescan();
        m
    }

    /// Load the built-ins and the presets folder again, as a plan does (AM8).
    pub fn rescan(&mut self) {
        self.registry = PresetRegistry::builtin().expect("the embedded presets load");
        self.skipped = self
            .dir
            .as_deref()
            .map(|d| self.registry.add_dir(d))
            .unwrap_or_default()
            .into_iter()
            .map(|w| w.message)
            .collect();
        self.refresh();
    }

    pub fn registry(&self) -> &PresetRegistry {
        &self.registry
    }

    /// Every selectable preset `(id, built-in)`, built-ins first.
    pub fn list(&self) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = self
            .registry
            .selectable()
            .into_iter()
            .map(|p| (p.id.clone(), self.registry.is_builtin(&p.id)))
            .collect();
        v.sort_by_key(|(id, b)| (!b, id.clone()));
        v
    }

    pub fn select(&mut self, id: &str) {
        let Some(t) = self.registry.table(id).cloned() else {
            return;
        };
        let builtin = self.registry.is_builtin(id);
        self.draft = Some(Draft {
            saved: t.clone(),
            table: t,
            orig: (!builtin).then(|| id.to_string()),
            read_only: builtin,
        });
        self.refresh();
    }

    fn unique_id(&self, stem: &str) -> String {
        (1..)
            .map(|n| {
                if n == 1 {
                    format!("{stem}-copy")
                } else {
                    format!("{stem}-copy{n}")
                }
            })
            .find(|id| self.registry.get(id).is_none())
            .expect("a free id")
    }

    /// An editable copy of the shown preset under a new id, without its suffix aliases (AM7: the copy and the
    /// original could not both load with them).
    pub fn duplicate(&mut self) {
        let Some(d) = &self.draft else { return };
        let mut t = d.table.clone();
        let id = self.unique_id(t.get("id").and_then(|v| v.as_str()).unwrap_or("preset"));
        t.insert("id".into(), id.into());
        t.remove("suffix_aliases");
        self.draft = Some(Draft {
            saved: toml::Table::new(),
            table: t,
            orig: None,
            read_only: false,
        });
        self.refresh();
    }

    /// A new preset on the target's quality preset.
    pub fn new_preset(&mut self, target: Target) {
        let mut t = toml::Table::new();
        t.insert("schema_version".into(), 1.into());
        t.insert("id".into(), "".into());
        t.insert("version".into(), "1.0.0".into());
        t.insert(
            "base".into(),
            engine::presets::default_preset_for(target).into(),
        );
        t.insert("target".into(), json_target(target).into());
        self.draft = Some(Draft {
            saved: toml::Table::new(),
            table: t,
            orig: None,
            read_only: false,
        });
        self.refresh();
    }

    fn str_field(&self, key: &str) -> String {
        self.draft
            .as_ref()
            .and_then(|d| d.table.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    pub fn id(&self) -> String {
        self.str_field("id")
    }

    pub fn base(&self) -> String {
        self.str_field("base")
    }

    /// The preset's own `target`, if it names one.
    pub fn own_target(&self) -> Option<Target> {
        serde_json::from_value(Value::String(self.str_field("target"))).ok()
    }

    fn edit(&mut self, f: impl FnOnce(&mut toml::Table)) {
        if let Some(d) = self.draft.as_mut().filter(|d| !d.read_only) {
            f(&mut d.table);
            self.refresh();
        }
    }

    pub fn set_id(&mut self, id: &str) {
        let id = id.to_string();
        self.edit(|t| {
            t.insert("id".into(), id.into());
        });
    }

    pub fn set_base(&mut self, base: &str) {
        let b = base.to_string();
        self.edit(|t| {
            t.insert("base".into(), b.into());
        });
    }

    /// `None` removes `target` (the base's then applies).
    pub fn set_target(&mut self, target: Option<Target>) {
        self.edit(|t| match target {
            Some(x) => {
                t.insert("target".into(), json_target(x).into());
            }
            None => {
                t.remove("target");
            }
        });
    }

    /// The draft's own value of `field` (a dotted profile path) for `st`, when it sets one.
    pub fn field(&self, st: Subtype, field: &str) -> Option<Value> {
        let d = self.draft.as_ref()?;
        let id = st.id();
        let mut path = vec!["subtypes", id.as_str()];
        path.extend(field.split('.'));
        get_at(&d.table, &path).map(json_of)
    }

    /// Set `field` for `st`. A format pick takes its tag (`value` is `[format, tag]`) and drops an own backend the
    /// new format cannot use.
    pub fn set_field(&mut self, st: Subtype, field: &str, value: Value) {
        let id = st.id();
        let field = field.to_string();
        let format = match (field.as_str(), &value) {
            ("format", Value::Array(p)) => serde_json::from_value::<Format>(p[0].clone()).ok(),
            _ => None,
        };
        self.edit(move |t| {
            let Some(sub) = table_at(t, &["subtypes", id.as_str()], true) else {
                return;
            };
            let mut put = |path: &str, v: &Value| {
                let parts: Vec<&str> = path.split('.').collect();
                let (last, parents) = parts.split_last().expect("a field path");
                if let (Some(tv), Some(parent)) =
                    (toml::Value::try_from(v).ok(), table_at(sub, parents, true))
                {
                    parent.insert((*last).into(), tv);
                }
            };
            if field == "format" {
                if let Value::Array(pair) = &value {
                    put("format", &pair[0]);
                    put("color.tag", &pair[1]);
                }
                let own_backend = get_at(sub, &["encoder", "backend"])
                    .and_then(|b| serde_json::from_value::<Backend>(json_of(b)).ok());
                if let (Some(b), Some(f)) = (own_backend, format) {
                    if !engine::policy::backend_fits(b, f) {
                        remove_at(sub, &["encoder", "backend"]);
                    }
                }
            } else {
                put(&field, &value);
            }
        });
    }

    /// ↺: drop the draft's own `field` for `st` (a format drops its tag too), so the base shows through.
    pub fn clear_field(&mut self, st: Subtype, field: &str) {
        let id = st.id();
        let fields: Vec<String> = if field == "format" {
            vec!["format".into(), "color.tag".into()]
        } else {
            vec![field.to_string()]
        };
        self.edit(move |t| {
            for f in &fields {
                let mut path = vec!["subtypes", id.as_str()];
                path.extend(f.split('.'));
                remove_at(t, &path);
            }
        });
    }

    /// Reset preset (AM9): drop every sub-type override (`[subtypes]`); names, aliases, remaps and rules stay.
    pub fn reset_all(&mut self) {
        self.edit(|t| {
            t.remove("subtypes");
        });
    }

    pub fn revert(&mut self) {
        if let Some(d) = self.draft.as_mut() {
            d.table = d.saved.clone();
        }
        self.refresh();
    }

    pub fn dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| d.table != d.saved)
    }

    /// The draft's aliases `(alias, canonical)` whose canonical suffix can name `st`.
    pub fn aliases_for(&self, st: Subtype) -> Vec<(String, String)> {
        let own = suffixes_of(st, self.target);
        self.aliases()
            .into_iter()
            .filter(|(_, c)| own.contains(&c.as_str()))
            .collect()
    }

    fn aliases(&self) -> Vec<(String, String)> {
        self.draft
            .as_ref()
            .and_then(|d| d.table.get("suffix_aliases"))
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The engine's verdict on `alias = "canonical"` beside every loaded preset (spec §3.7 `check_aliases`): a
    /// probe preset holding only the alias is loaded into a copy of the registry without the edited preset.
    pub fn check_alias(&self, alias: &str, canonical: &str) -> Result<(), String> {
        if self.aliases().iter().any(|(a, _)| a == alias) {
            return Err(format!("'{alias}' is already an alias of this preset"));
        }
        let mut r = self.registry.clone();
        if let Some(o) = self.draft.as_ref().and_then(|d| d.orig.as_deref()) {
            r.remove(o);
        }
        let mut probe = toml::Table::new();
        probe.insert("schema_version".into(), 1.into());
        probe.insert("id".into(), "this preset".into());
        probe.insert("version".into(), "1.0.0".into());
        let mut a = toml::Table::new();
        a.insert(alias.into(), canonical.into());
        probe.insert("suffix_aliases".into(), toml::Value::Table(a));
        r.add_toml(&toml::to_string(&probe).unwrap_or_default())
            .map(|_| ())
            .map_err(|e| {
                let m = e.to_string();
                m.strip_prefix("CFG-INVALID: preset 'this preset': ")
                    .unwrap_or(&m)
                    .to_string()
            })
    }

    pub fn add_alias(&mut self, alias: &str, canonical: &str) -> Result<(), String> {
        self.check_alias(alias, canonical)?;
        let (a, c) = (alias.to_string(), canonical.to_string());
        self.edit(|t| {
            if let Some(s) = table_at(t, &["suffix_aliases"], true) {
                s.insert(a, c.into());
            }
        });
        Ok(())
    }

    /// Remove an alias and its remap rule.
    pub fn remove_alias(&mut self, alias: &str) {
        let glob = remap_glob(alias);
        let a = alias.to_string();
        self.edit(|t| {
            remove_at(t, &["suffix_aliases", a.as_str()]);
            if let Some(rules) = t.get_mut("rules").and_then(toml::Value::as_array_mut) {
                rules.retain(|r| remap_of(r).is_none_or(|(g, _)| g != glob));
                if rules.is_empty() {
                    t.remove("rules");
                }
            }
        });
    }

    /// The draft's swizzle remaps `(alias, canonical, swizzle)`: an alias with its own glob rule (AM10).
    pub fn remaps(&self) -> Vec<(String, String, String)> {
        let rules = self
            .draft
            .as_ref()
            .and_then(|d| d.table.get("rules"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        self.aliases()
            .into_iter()
            .filter_map(|(a, c)| {
                let glob = remap_glob(&a);
                let s = rules
                    .iter()
                    .filter_map(remap_of)
                    .find(|(g, _)| *g == glob)?
                    .1;
                Some((a, c, s))
            })
            .collect()
    }

    /// A swizzle remap: the alias plus a rule `match = { glob = "*_<alias>.*" }` that sets `channels.swizzle`.
    pub fn add_remap(&mut self, alias: &str, canonical: &str, swizzle: &str) -> Result<(), String> {
        if swizzle.len() != 4 || !swizzle.chars().all(|c| SWIZZLE_SOURCES.contains(&c)) {
            return Err(format!("'{swizzle}': four of r, g, b, a, 0, 1"));
        }
        self.add_alias(alias, canonical)?;
        let rule: toml::Table = toml::from_str(&format!(
            "match = {{ glob = \"{}\" }}\noverrides = {{ channels = {{ swizzle = \"{swizzle}\" }} }}",
            remap_glob(alias)
        ))
        .expect("a remap rule");
        self.edit(|t| {
            let rules = t
                .entry("rules")
                .or_insert_with(|| toml::Value::Array(vec![]));
            if let Some(a) = rules.as_array_mut() {
                a.push(toml::Value::Table(rule));
            }
        });
        Ok(())
    }

    /// Exactly what Save writes.
    pub fn toml_text(&self) -> String {
        self.draft
            .as_ref()
            .map(|d| toml::to_string(&d.table).unwrap_or_default())
            .unwrap_or_default()
    }

    /// `Ok` when the draft loads through the engine; else the first problem.
    pub fn status(&self) -> &Result<(), String> {
        &self.status
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn row(&self, st: Subtype) -> Option<&Row> {
        self.rows.iter().find(|r| r.subtype == st)
    }

    /// The draft through the engine loader: its id rule (AM6), then `add_toml` and `check_preset` into a copy of the
    /// registry without the preset it edits.
    fn validate(&self) -> Result<String, String> {
        let d = self.draft.as_ref().ok_or("no preset")?;
        check_id(&self.id())?;
        let text = toml::to_string(&d.table).map_err(|e| e.to_string())?;
        let mut r = self.registry.clone();
        if let Some(o) = &d.orig {
            r.remove(o);
        }
        let id = r.add_toml(&text).map_err(|e| e.to_string())?;
        r.check_preset(&id).map_err(|e| e.to_string())?;
        Ok(text)
    }

    fn refresh(&mut self) {
        self.error = None;
        let Some(d) = &self.draft else {
            self.rows.clear();
            self.status = Ok(());
            return;
        };
        let parsed: Result<PresetFile, String> = toml::Value::Table(d.table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string());
        let base = self.base();
        let chain = if base.is_empty() {
            Ok(vec![])
        } else {
            self.registry.chain(&base).map_err(|e| e.to_string())
        };
        self.target = self
            .own_target()
            .or_else(|| {
                chain
                    .as_ref()
                    .ok()
                    .and_then(|c| c.iter().rev().find_map(|p| p.target))
            })
            .unwrap_or(self.fallback);
        let target = self.target;
        self.rows = Subtype::ALL
            .into_iter()
            .map(|st| {
                let profile = match (&parsed, &chain) {
                    (Ok(p), Ok(c)) => {
                        let mut all = c.clone();
                        all.push(p);
                        let mut doc = Value::Object(Map::new());
                        for (_, patch) in PresetRegistry::layers_of(&all, st) {
                            merge_json(&mut doc, &patch);
                        }
                        finish_profile(doc, target, st)
                            .map(follow_backend)
                            .map_err(|e| e.to_string())
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e.clone()),
                };
                let base = (!base.is_empty())
                    .then(|| self.registry.profile(&base, target, st).ok())
                    .flatten()
                    .map(follow_backend);
                Row {
                    subtype: st,
                    profile,
                    base,
                }
            })
            .collect();
        self.status = self.validate().map(|_| ());
    }

    /// A user preset loaded from a file: Delete is offered.
    pub fn deletable(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| d.orig.is_some())
    }

    /// Save (AM8): validate, write the preset's own file (a new preset: `<folder>/<id>.toml`, never over an existing
    /// file), then rescan the folder and show the saved preset.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let dir = self
            .dir
            .clone()
            .ok_or("No presets folder (APPDATA is not set)")?;
        let text = self.validate()?;
        let id = self.id();
        let orig = self.draft.as_ref().and_then(|d| d.orig.clone());
        let path = match orig.as_deref().and_then(|o| self.registry.source(o)) {
            Some(src) => PathBuf::from(src),
            None => {
                let p = dir.join(format!("{id}.toml"));
                if p.exists() {
                    return Err(format!(
                        "{} already exists (a preset that did not load?): choose another name",
                        p.display()
                    ));
                }
                p
            }
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.rescan();
        self.select(&id);
        Ok(path)
    }

    /// Delete the shown user preset's file, then rescan.
    pub fn delete(&mut self) -> Result<(), String> {
        let orig = self
            .draft
            .as_ref()
            .and_then(|d| d.orig.clone())
            .ok_or("only a user preset can be deleted")?;
        let src = self
            .registry
            .source(&orig)
            .ok_or("the preset is not loaded")?
            .to_string();
        std::fs::remove_file(&src).map_err(|e| format!("{src}: {e}"))?;
        self.draft = None;
        self.rescan();
        Ok(())
    }
}

/// `cs`, `vanilla`, `cpm`.
fn json_target(t: Target) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The engine's resolve moves a backend that cannot write the format to the format's own (`backend_follows_format`);
/// the rows show the same.
fn follow_backend(mut p: Profile) -> Profile {
    if !engine::policy::backend_fits(p.encoder.backend, p.format) {
        p.encoder.backend = engine::policy::default_backend(p.format);
    }
    p
}

/// `(glob, swizzle)` of a remap rule: a glob-only match and a swizzle-only override.
fn remap_of(rule: &toml::Value) -> Option<(String, String)> {
    let r = rule.as_table()?;
    let m = r.get("match")?.as_table()?;
    let o = r.get("overrides")?.as_table()?;
    let ch = o.get("channels")?.as_table()?;
    (r.len() == 2 && m.len() == 1 && o.len() == 1 && ch.len() == 1).then_some(())?;
    Some((
        m.get("glob")?.as_str()?.to_string(),
        ch.get("swizzle")?.as_str()?.to_string(),
    ))
}
