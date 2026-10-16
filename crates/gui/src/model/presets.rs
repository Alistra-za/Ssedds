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
G = metallic from its B, B = AO from its R; specular (A) comes from a loose _specular map when the set has one (it wins over the fill), else packing.fill.specular, else 1.";

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

/// The top-level families (AP9), in order, with their hint.
pub const FAMILIES: [(&str, &str); 3] = [
    ("PBR / Community Shaders", "used under the cs target"),
    (
        "Complex Parallax Materials (cpm)",
        "complex parallax materials (used under every target since vanilla keeps a complex _m)",
    ),
    ("Vanilla", "used under every target"),
];

/// AP9: the most specific family a sub-type is available in, from `policy::availability`: cs only → PBR / CS,
/// cs and cpm only → CPM, anything vanilla also has → Vanilla. complex_grass is placed in PBR / CS and
/// complex_material in CPM by hand.
pub fn family_of(st: Subtype) -> &'static str {
    use engine::policy::{availability, Availability};
    // Owner ruling (AP9): available under every target, but a Community Shaders feature in practice.
    if st == Subtype::ComplexGrass {
        return FAMILIES[0].0;
    }
    // AP12: vanilla keeps a complex `_m` since AT1, which would empty CPM; the owner's AP9 family stays.
    if st == Subtype::ComplexMaterial {
        return FAMILIES[1].0;
    }
    let on = |t| availability(t, st) == Availability::Available;
    let i = match (on(Target::Cpm), on(Target::Vanilla)) {
        (false, false) => 0,
        (true, false) => 1,
        _ => 2,
    };
    FAMILIES[i].0
}

/// The body's layout: `(family, group, sub-types)` in family order, then `GROUPS` order, then `Subtype::ALL`
/// order; empty groups left out.
pub fn layout() -> Vec<(&'static str, &'static str, Vec<Subtype>)> {
    FAMILIES
        .iter()
        .flat_map(|(f, _)| GROUPS.iter().map(move |g| (*f, *g)))
        .map(|(f, g)| {
            let sts = Subtype::ALL
                .into_iter()
                .filter(|st| family_of(*st) == f && group_of(*st) == g)
                .collect();
            (f, g, sts)
        })
        .filter(|(_, _, s): &(_, _, Vec<Subtype>)| !s.is_empty())
        .collect()
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

/// The glob of an alias's branch rule (AM10, AP3): a glob without '/' matches the file name alone.
fn remap_glob(alias: &str) -> String {
    format!("*_{alias}.*")
}

/// One sub-type's row: the profile the draft resolves to and the base's, under the draft's target.
pub struct Row {
    pub subtype: Subtype,
    pub profile: Result<Profile, String>,
    pub base: Option<Profile>,
    /// Note AP: one branch per suffix that names the sub-type, built-in suffixes first, then the draft's aliases.
    pub branches: Vec<Branch>,
}

/// A suffix branch (Note AP): the sub-type's profile with the branch rule's overrides on top.
pub struct Branch {
    /// The canonical suffix or the alias, without '_'.
    pub suffix: String,
    pub alias: bool,
    /// What a file with this suffix gets (AP11): the row's document, then every rule a plan would apply to it in
    /// order (the chain's and the draft's, so a canonical suffix's branch under an alias's), this branch included.
    pub profile: Result<Profile, String>,
    /// The same without this branch's own rule: what its fields' hints show.
    pub inherited: Result<Profile, String>,
    /// A rule on alpha content may also apply; it is left out of `profile`.
    pub by_alpha: bool,
    /// The fields the branch sets (a format and its tag count once).
    pub overrides: usize,
}

/// What is known of a file a branch addresses: its sub-type, its canonical suffix, and the alias it is named with.
struct BranchFacts<'a> {
    st: Subtype,
    canon: &'a str,
    alias: Option<&'a str>,
}

impl BranchFacts<'_> {
    /// Whether `r` applies to such a file (`policy::rule_matches` on these facts): `None` when it turns on alpha
    /// content. A rule on a path (other than the alias's glob) or a set's members cannot be known and is left out.
    fn applies(&self, r: &engine::presets::Rule) -> Option<bool> {
        let m = &r.matcher;
        let known = r.subtype.is_none_or(|s| s == self.st)
            && (m.subtypes.is_empty() || m.subtypes.contains(&self.st))
            && m.suffix
                .as_deref()
                .is_none_or(|s| s.eq_ignore_ascii_case(self.canon))
            && m.glob
                .as_deref()
                .is_none_or(|g| self.alias.is_some_and(|a| g == remap_glob(a)))
            && m.set_has.is_none();
        match (known, m.alpha) {
            (true, Some(_)) => None,
            (k, _) => Some(k),
        }
    }

    /// The branch's own rule (AP3's shape for this sub-type and suffix).
    fn is_own(&self, r: &engine::presets::Rule) -> bool {
        let m = &r.matcher;
        let shape = r.subtype.is_none()
            && m.subtypes == [self.st]
            && m.set_has.is_none()
            && m.alpha.is_none();
        shape
            && match self.alias {
                Some(a) => m.suffix.is_none() && m.glob.as_deref() == Some(remap_glob(a).as_str()),
                None => m.glob.is_none() && m.suffix.as_deref() == Some(self.canon),
            }
    }
}

/// The preset being shown.
pub struct Draft {
    pub table: toml::Table,
    saved: toml::Table,
    /// The id of the loaded user preset this edits; `None` for a new preset or a duplicate.
    orig: Option<String>,
    /// A built-in, shown read-only (Duplicate makes an editable copy).
    pub read_only: bool,
    /// The text of the file a user preset was loaded from: Save edits it in place (comments and order kept).
    source: Option<String>,
    /// The stamp of that file when it was picked (audit item 4): Save refuses when it changed.
    stamp: Option<engine::policy::SourceStamp>,
    /// That file: Save writes it, even after a rescan no longer finds the preset there.
    file: Option<PathBuf>,
}

pub struct PresetsModel {
    dir: Option<PathBuf>,
    registry: PresetRegistry,
    /// `(file, reason)` of the presets folder files that did not load (CFG-PRESET-SKIPPED).
    pub skipped: Vec<(String, String)>,
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
    /// The last Save was refused because the preset file changed on disk (audit item 4): the window asks
    /// Overwrite / Reload / Cancel. Cleared by the next change.
    pub changed_on_disk: bool,
    /// `suffixes_of` for every sub-type under `.0` (thousands of `suggest` calls, so once per target).
    suffixes: (Option<Target>, Vec<Vec<&'static str>>),
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
            changed_on_disk: false,
            suffixes: (None, vec![]),
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
            .map(|w| {
                let s = |k: &str| w.data[k].as_str().unwrap_or_default().to_string();
                (s("file"), s("reason"))
            })
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
        let file = (!builtin).then(|| self.registry.source(id)).flatten();
        self.draft = Some(Draft {
            saved: t.clone(),
            table: t,
            orig: (!builtin).then(|| id.to_string()),
            read_only: builtin,
            stamp: file.and_then(|p| engine::policy::SourceStamp::of(p.as_ref())),
            source: file.and_then(|p| std::fs::read_to_string(p).ok()),
            file: file.map(PathBuf::from),
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
        // The aliases go, and so do their branch and remap rules (a rule for an alias the copy lacks would be an
        // orphan).
        let aliases: Vec<String> = t
            .remove("suffix_aliases")
            .and_then(|v| v.as_table().map(|a| a.keys().cloned().collect()))
            .unwrap_or_default();
        for a in aliases {
            drop_alias_rules(&mut t, &a);
        }
        self.draft = Some(Draft {
            saved: toml::Table::new(),
            table: t,
            orig: None,
            read_only: false,
            source: None,
            stamp: None,
            file: None,
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
            source: None,
            stamp: None,
            file: None,
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
        self.scoped_field(st, None, field)
    }

    /// The branch `suffix` of `st`'s own value of `field` (Note AP).
    pub fn branch_field(&self, st: Subtype, suffix: &str, field: &str) -> Option<Value> {
        self.scoped_field(st, Some(suffix), field)
    }

    fn scoped_field(&self, st: Subtype, branch: Option<&str>, field: &str) -> Option<Value> {
        let path: Vec<&str> = field.split('.').collect();
        get_at(self.scope(st, branch)?, &path).map(json_of)
    }

    /// The table edits of `st` land in: `[subtypes.<id>]`, or the branch rule's `overrides`.
    fn scope(&self, st: Subtype, branch: Option<&str>) -> Option<&toml::Table> {
        let t = &self.draft.as_ref()?.table;
        match branch {
            None => t.get("subtypes")?.get(st.id())?.as_table(),
            Some(s) => {
                let i = self.branch_index(st, s)?;
                t.get("rules")?.get(i)?.get("overrides")?.as_table()
            }
        }
    }

    /// Set `field` for `st`. A format pick takes its tag (`value` is `[format, tag]`) and drops an own backend the
    /// new format cannot use.
    pub fn set_field(&mut self, st: Subtype, field: &str, value: Value) {
        self.edit_scope(st, None, |sub| put_field(sub, field, &value));
    }

    /// Set `field` in `st`'s branch `suffix`, creating its rule (Note AP).
    pub fn set_branch_field(&mut self, st: Subtype, suffix: &str, field: &str, value: Value) {
        self.edit_scope(st, Some(suffix), |sub| put_field(sub, field, &value));
    }

    /// ↺: drop the draft's own `field` for `st` (a format drops its tag too), so the base shows through.
    pub fn clear_field(&mut self, st: Subtype, field: &str) {
        self.edit_scope(st, None, |sub| drop_field(sub, field));
    }

    /// ↺ in a branch: an emptied branch's rule goes (Note AP).
    pub fn clear_branch_field(&mut self, st: Subtype, suffix: &str, field: &str) {
        self.edit_scope(st, Some(suffix), |sub| drop_field(sub, field));
    }

    /// Run `f` on `st`'s scope table (created as needed), then drop what it left empty. A preset's matching rules layer in
    /// file order (AP10), so a new built-in suffix branch goes after the other rules and before the alias branches, and
    /// a new alias branch goes last: it adds to its canonical suffix's branch (AP2).
    fn edit_scope(&mut self, st: Subtype, branch: Option<&str>, f: impl FnOnce(&mut toml::Table)) {
        let id = st.id();
        let Some(sfx) = branch else {
            return self.edit(|t| {
                if let Some(sub) = table_at(t, &["subtypes", id.as_str()], true) {
                    f(sub);
                    if sub.is_empty() {
                        remove_at(t, &["subtypes", id.as_str()]);
                    }
                }
            });
        };
        let alias = self.aliases().iter().any(|(a, _)| a == sfx);
        let at = self.branch_index(st, sfx);
        let rules = self.rules();
        let insert = if alias {
            rules.len()
        } else {
            // After the last hand-written rule, then before the first alias branch that follows it.
            let start = rules
                .iter()
                .rposition(|r| self.branch_key(r).is_none())
                .map_or(0, |i| i + 1);
            rules[start..]
                .iter()
                .position(|r| self.branch_key(r).is_some_and(|(_, _, a)| a))
                .map_or(rules.len(), |i| start + i)
        };
        let rule = branch_rule(&id, sfx, alias);
        self.edit(move |t| {
            let rules = t
                .entry("rules")
                .or_insert_with(|| toml::Value::Array(vec![]));
            let Some(a) = rules.as_array_mut() else {
                return;
            };
            let i = at.unwrap_or_else(|| {
                a.insert(insert, rule);
                insert
            });
            if let Some(o) = a[i]
                .get_mut("overrides")
                .and_then(toml::Value::as_table_mut)
            {
                f(o);
            }
            if a[i]
                .get("overrides")
                .and_then(toml::Value::as_table)
                .is_none_or(toml::Table::is_empty)
            {
                a.remove(i);
            }
            if a.is_empty() {
                t.remove("rules");
            }
        });
    }

    fn rules(&self) -> Vec<toml::Value> {
        self.draft
            .as_ref()
            .and_then(|d| d.table.get("rules"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    }

    /// The suffixes that can name `st` under the draft's target (`suffixes_of`, cached).
    pub fn suffixes(&self, st: Subtype) -> Vec<&'static str> {
        let i = Subtype::ALL
            .iter()
            .position(|s| *s == st)
            .expect("a sub-type");
        self.suffixes.1.get(i).cloned().unwrap_or_default()
    }

    /// `(sub-type, suffix, alias)` when `rule` is a branch the editor owns and shows (AP3): exactly `match` and
    /// `overrides`, `match` being one sub-type plus a suffix that names it or an alias glob (`*_<alias>.*`) of one of
    /// its aliases.
    fn branch_key(&self, rule: &toml::Value) -> Option<(Subtype, String, bool)> {
        let r = rule.as_table()?;
        let m = r.get("match")?.as_table()?;
        r.get("overrides")?.as_table()?;
        let sts = m.get("subtypes")?.as_array()?;
        (r.len() == 2 && m.len() == 2 && sts.len() == 1).then_some(())?;
        let st: Subtype =
            serde_json::from_value(Value::String(sts[0].as_str()?.to_string())).ok()?;
        if let Some(s) = m.get("suffix") {
            let s = s.as_str()?;
            return self
                .suffixes(st)
                .contains(&s)
                .then(|| (st, s.to_string(), false));
        }
        let g = m.get("glob")?.as_str()?;
        self.aliases_for(st)
            .into_iter()
            .find(|(a, _)| remap_glob(a) == g)
            .map(|(a, _)| (st, a, true))
    }

    fn branch_index(&self, st: Subtype, suffix: &str) -> Option<usize> {
        self.rules().iter().position(|r| {
            self.branch_key(r)
                .is_some_and(|(s, x, _)| s == st && x == suffix)
        })
    }

    /// The draft's rules that are not branches, as TOML: kept as they are and shown read-only (AP4).
    pub fn hand_rules(&self) -> Vec<String> {
        self.rules()
            .iter()
            .filter(|r| self.branch_key(r).is_none())
            .map(|r| {
                r.as_table()
                    .and_then(|t| toml::to_string(t).ok())
                    .unwrap_or_else(|| r.to_string())
            })
            .collect()
    }

    /// Reset preset (AM9, AP5): drop every sub-type override (`[subtypes]`) and every branch rule; names, aliases and
    /// other rules stay.
    pub fn reset_all(&mut self) {
        let keep: Vec<toml::Value> = self
            .rules()
            .into_iter()
            .filter(|r| self.branch_key(r).is_none())
            .collect();
        self.edit(|t| {
            t.remove("subtypes");
            if keep.is_empty() {
                t.remove("rules");
            } else {
                t.insert("rules".into(), toml::Value::Array(keep));
            }
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
        let own = self.suffixes(st);
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

    /// Remove an alias and its branch rules (and an AM10 remap rule).
    pub fn remove_alias(&mut self, alias: &str) {
        self.edit(|t| {
            remove_at(t, &["suffix_aliases", alias]);
            drop_alias_rules(t, alias);
        });
    }

    /// Exactly what Save writes.
    pub fn toml_text(&self) -> String {
        self.draft
            .as_ref()
            .map(|d| output(d).unwrap_or_else(|e| e))
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
        let text = output(d)?;
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
        self.changed_on_disk = false;
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
        if self.suffixes.0 != Some(target) {
            self.suffixes = (
                Some(target),
                Subtype::ALL.map(|st| suffixes_of(st, target)).to_vec(),
            );
        }
        // Every rule a plan sees for this preset, in order: the chain's (base first), then the draft's (AP11).
        let rules: Vec<&engine::presets::Rule> = match (&parsed, &chain) {
            (Ok(p), Ok(c)) => c.iter().flat_map(|x| &x.rules).chain(&p.rules).collect(),
            _ => vec![],
        };
        let rows = Subtype::ALL
            .into_iter()
            .map(|st| {
                let doc = match (&parsed, &chain) {
                    (Ok(p), Ok(c)) => {
                        let mut all = c.clone();
                        all.push(p);
                        let mut doc = Value::Object(Map::new());
                        for (_, patch) in PresetRegistry::layers_of(&all, st) {
                            merge_json(&mut doc, &patch);
                        }
                        Ok(doc)
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e.clone()),
                };
                let finish = |doc: Value| {
                    finish_profile(doc, target, st)
                        .map(follow_backend)
                        .map_err(|e| e.to_string())
                };
                let branches = self
                    .suffixes(st)
                    .into_iter()
                    .map(|s| (s.to_string(), s.to_string(), false))
                    .chain(self.aliases_for(st).into_iter().map(|(a, c)| (a, c, true)))
                    .map(|(suffix, canon, alias)| {
                        let own = self.scope(st, Some(&suffix));
                        let facts = BranchFacts {
                            st,
                            canon: &canon,
                            alias: alias.then_some(suffix.as_str()),
                        };
                        let layered = |with_own: bool| {
                            doc.clone().map(|mut d| {
                                let mut by_alpha = false;
                                for r in &rules {
                                    match facts.applies(r) {
                                        Some(true) if with_own || !facts.is_own(r) => {
                                            merge_json(&mut d, &r.overrides)
                                        }
                                        None => by_alpha = true,
                                        _ => {}
                                    }
                                }
                                (d, by_alpha)
                            })
                        };
                        let by_alpha = layered(true).is_ok_and(|(_, a)| a);
                        Branch {
                            overrides: own.map_or(0, count_fields),
                            profile: layered(true).and_then(|(d, _)| finish(d)),
                            inherited: layered(false).and_then(|(d, _)| finish(d)),
                            by_alpha,
                            suffix,
                            alias,
                        }
                    })
                    .collect();
                let base = (!base.is_empty())
                    .then(|| self.registry.profile(&base, target, st).ok())
                    .flatten()
                    .map(follow_backend);
                Row {
                    subtype: st,
                    profile: doc.and_then(finish),
                    base,
                    branches,
                }
            })
            .collect();
        self.rows = rows;
        self.status = self.validate().map(|_| ());
    }

    /// A user preset loaded from a file: Delete is offered.
    pub fn deletable(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| d.orig.is_some())
    }

    /// Save (AM8): validate, write the preset's own file (a new preset: `<folder>/<id>.toml`, never over an existing
    /// file), then rescan the folder and show the saved preset.
    /// A user preset's file that changed on disk since it was picked is not written: `changed_on_disk` is set and
    /// the window asks (audit item 4).
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.save_with(false)
    }

    /// The answer Overwrite: save over a preset file that changed on disk.
    pub fn save_overwrite(&mut self) -> Result<PathBuf, String> {
        self.save_with(true)
    }

    /// The answer Reload: drop the draft and pick the preset again from its file.
    /// A file that was deleted or no longer loads keeps the draft (and the question), with a note.
    pub fn reload_from_disk(&mut self) {
        let Some(d) = &self.draft else { return };
        let (orig, file) = (d.orig.clone(), d.file.clone());
        self.rescan();
        let Some(id) = orig else { return };
        if self.registry.table(&id).is_some() {
            self.select(&id);
            return;
        }
        let name = file
            .as_deref()
            .map_or(id.clone(), |f| f.display().to_string());
        let why = if file.as_deref().is_some_and(std::path::Path::exists) {
            "no longer loads"
        } else {
            "was deleted"
        };
        self.error = Some(format!("{name} {why}: kept your edits"));
        self.changed_on_disk = true;
    }

    fn save_with(&mut self, force: bool) -> Result<PathBuf, String> {
        let dir = self
            .dir
            .clone()
            .ok_or("No presets folder (APPDATA is not set)")?;
        let text = self.validate()?;
        let id = self.id();
        let orig = self.draft.as_ref().and_then(|d| d.orig.clone());
        let file = self.draft.as_ref().and_then(|d| d.file.clone());
        let path = match orig.as_ref().and(file) {
            Some(path) => {
                let stamp = self.draft.as_ref().and_then(|d| d.stamp);
                if !force && engine::policy::SourceStamp::of(&path) != stamp {
                    self.changed_on_disk = true;
                    return Err(format!(
                        "{} changed on disk since it was loaded",
                        path.display()
                    ));
                }
                path
            }
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
        engine::dds::write_atomic(&path, text.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        self.rescan();
        self.select(&id);
        Ok(path)
    }

    /// The file the shown user preset was loaded from (what Delete removes).
    pub fn source_file(&self) -> Option<String> {
        let orig = self.draft.as_ref()?.orig.as_deref()?;
        self.registry.source(orig).map(str::to_string)
    }

    /// The presets folder, if there is one.
    pub fn dir(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
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

/// Write `field` into a scope table. A format pick (`[format, tag]`) writes both and drops an own backend the new
/// format cannot use.
fn put_field(sub: &mut toml::Table, field: &str, value: &Value) {
    let put = |sub: &mut toml::Table, path: &str, v: &Value| {
        let parts: Vec<&str> = path.split('.').collect();
        let (last, parents) = parts.split_last().expect("a field path");
        if let (Some(tv), Some(parent)) =
            (toml::Value::try_from(v).ok(), table_at(sub, parents, true))
        {
            parent.insert((*last).into(), tv);
        }
    };
    let (Value::Array(pair), "format") = (value, field) else {
        return put(sub, field, value);
    };
    put(sub, "format", &pair[0]);
    put(sub, "color.tag", &pair[1]);
    let own_backend = get_at(sub, &["encoder", "backend"])
        .and_then(|b| serde_json::from_value::<Backend>(json_of(b)).ok());
    let format = serde_json::from_value::<Format>(pair[0].clone()).ok();
    if let (Some(b), Some(f)) = (own_backend, format) {
        if !engine::policy::backend_fits(b, f) {
            remove_at(sub, &["encoder", "backend"]);
        }
    }
}

/// Remove `field` from a scope table (a format takes its tag with it) and the tables it leaves empty.
fn drop_field(sub: &mut toml::Table, field: &str) {
    if field == "format" {
        remove_at(sub, &["color", "tag"]);
    }
    remove_at(sub, &field.split('.').collect::<Vec<_>>());
}

/// A new, empty branch rule (AP3): a built-in suffix matches `match.suffix`; an alias matches its glob, since
/// `match.suffix` sees the canonical suffix.
fn branch_rule(st_id: &str, suffix: &str, alias: bool) -> toml::Value {
    let mut m = toml::Table::new();
    if alias {
        m.insert("glob".into(), remap_glob(suffix).into());
    } else {
        m.insert("suffix".into(), suffix.into());
    }
    m.insert("subtypes".into(), toml::Value::Array(vec![st_id.into()]));
    let mut r = toml::Table::new();
    r.insert("match".into(), toml::Value::Table(m));
    r.insert("overrides".into(), toml::Value::Table(toml::Table::new()));
    toml::Value::Table(r)
}

/// Drop the rules matched only by `alias`'s glob (its branches, and an AM10 remap rule).
fn drop_alias_rules(t: &mut toml::Table, alias: &str) {
    let glob = remap_glob(alias);
    let Some(rules) = t.get_mut("rules").and_then(toml::Value::as_array_mut) else {
        return;
    };
    rules.retain(|r| {
        let m = r.get("match").and_then(toml::Value::as_table);
        !m.is_some_and(|m| {
            m.get("glob").and_then(toml::Value::as_str) == Some(glob.as_str())
                && m.keys().all(|k| k == "glob" || k == "subtypes")
        })
    });
    if rules.is_empty() {
        t.remove("rules");
    }
}

/// The fields a branch sets: its leaf values, a format's tag not counted apart.
fn count_fields(t: &toml::Table) -> usize {
    fn leaves(t: &toml::Table) -> usize {
        t.values().map(|v| v.as_table().map_or(1, leaves)).sum()
    }
    let tag_with_format = t.contains_key("format") && get_at(t, &["color", "tag"]).is_some();
    leaves(t) - usize::from(tag_with_format)
}

/// The text Save writes: a user preset's own file edited in place (`toml_edit`, so its comments, key order and
/// layout stay where the values did not change), else the table serialised (AM13).
fn output(d: &Draft) -> Result<String, String> {
    let Some(src) = &d.source else {
        return toml::to_string(&d.table).map_err(|e| e.to_string());
    };
    let mut doc: toml_edit::DocumentMut = src.parse().map_err(|e| format!("{e}"))?;
    let old: toml::Table = toml::from_str(src).map_err(|e| e.to_string())?;
    sync(doc.as_table_mut(), false, &old, &d.table)?;
    Ok(doc.to_string())
}

/// Make `doc` (whose values are `old`) hold `new`, touching only the keys that differ: a table on both sides is
/// synced key by key, anything else that changed is replaced.
/// `inline`: `doc` is an inline table, so what goes in must be a value.
fn sync(
    doc: &mut dyn toml_edit::TableLike,
    inline: bool,
    old: &toml::Table,
    new: &toml::Table,
) -> Result<(), String> {
    let gone: Vec<String> = old
        .keys()
        .filter(|k| !new.contains_key(*k))
        .cloned()
        .collect();
    for k in gone {
        doc.remove(&k);
    }
    for (k, v) in new {
        if old.get(k) == Some(v) {
            continue;
        }
        if let (Some(toml::Value::Table(o)), toml::Value::Table(n)) = (old.get(k), v) {
            let sub_inline = inline || doc.get(k).is_some_and(toml_edit::Item::is_value);
            if let Some(t) = doc.get_mut(k).and_then(toml_edit::Item::as_table_like_mut) {
                sync(t, sub_inline, o, n)?;
                continue;
            }
        }
        let item = item_of(k, v)?;
        let item = if inline {
            toml_edit::Item::Value(item.into_value().map_err(|_| format!("{k}: not a value"))?)
        } else {
            item
        };
        doc.insert(k, item);
    }
    Ok(())
}

/// `v` as a `toml_edit` item, laid out as `toml` writes it (a table as a `[section]`, rules as `[[rules]]`).
fn item_of(k: &str, v: &toml::Value) -> Result<toml_edit::Item, String> {
    let mut t = toml::Table::new();
    t.insert(k.to_string(), v.clone());
    let text = toml::to_string(&t).map_err(|e| e.to_string())?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
    doc.as_table_mut()
        .remove(k)
        .ok_or_else(|| "a serialised value".to_string())
}
