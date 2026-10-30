//! Export of PBR sets (M3b spec §6): the checks that block or ask, and the JSON entries. No egui types.
use super::bottom::Fix;
use super::export::{Row, RowState};
use super::history::{recorded, sha256_of};
use super::sets::{
    engine_inputs, item_output_key, output_candidates, set_views, SetView, SlotFill, SlotKind,
};
use super::{AppModel, ViewCache};
use crate::pgp::prefix::{
    check_json_path, check_rel_path, matches_nothing, output_rel, pitfalls, rename_hits_elsewhere,
    subsurface_pitfalls,
};
use crate::pgp::settings::{blocked_by, feature_key, feature_on, Feature, Settings, FEATURE_KEYS};
use crate::pgp::validate::validate;
use crate::pgp::writer::{document, to_text, EntryInput};
use engine::config::Target;
use engine::sets::{same_id, OutputKey};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportIssue {
    pub set_id: String,
    /// The engine key, for selecting the set (empty for a created set with no members, and for project-wide issues).
    pub set_key: String,
    pub text: String,
    /// Note FC3: the Problems row's action (P13 `Fix path`, P26 `Turn {feature} off`).
    pub fix: Option<Fix>,
}

fn issue(v: &SetView, text: String) -> ExportIssue {
    ExportIssue {
        set_id: v.id.clone(),
        set_key: v.key.clone(),
        text,
        fix: None,
    }
}

fn general(text: String) -> ExportIssue {
    ExportIssue {
        set_id: String::new(),
        set_key: String::new(),
        text,
        fix: None,
    }
}

/// The JSON file name (without `.json`): the project's, else the opened folder's name.
pub fn json_name(m: &AppModel) -> String {
    m.sets_doc
        .as_ref()
        .and_then(|d| d.json_name())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| {
            m.root
                .as_ref()
                .and_then(|r| r.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "sdc".into())
}

/// The output path of the texture filling a slot, when it has one.
fn fill_path<'a>(
    item: &str,
    views: &[SetView],
    paths: &'a BTreeMap<OutputKey, PathBuf>,
    m: &AppModel,
) -> Option<&'a PathBuf> {
    m.item(item)
        .and_then(|it| item_output_key(views, it))
        .and_then(|k| paths.get(&k))
}

/// The final slot paths of a set: a slot whose finished file lives at another set's path points there (a borrowed
/// slot, or the set's own texture when a set earlier in the order owns it, spec §3.3); an explicit `slotN` setting
/// wins.
fn slots_of(
    v: &SetView,
    views: &[SetView],
    paths: &BTreeMap<OutputKey, PathBuf>,
    m: &AppModel,
) -> BTreeMap<u8, String> {
    let mut out = BTreeMap::new();
    for (kind, fill) in &v.slots {
        let (item, borrowed) = match fill {
            SlotFill::Borrowed { item, .. } => (item, true),
            SlotFill::Own { item } => (item, false),
            SlotFill::Empty => continue,
        };
        let Some(p) = fill_path(item, views, paths, m) else {
            continue;
        };
        // PGPatcher finds the set's own texture at `<prefix>_<suffix>.dds` without a slot command. Its slot 3 default
        // is not known to look for `_sk`, so a skin `_sk` is always named (final review M5): an explicit slot is
        // never wrong. An `_rfaos` is not at the RMAOS slot's own path, so slot 6 is named too.
        let at_own_path = *kind != SlotKind::SkinSk
            && v.prefix
                .as_deref()
                .is_some_and(|pre| path_lower(p) == path_lower(&output_rel(pre, kind.suffix())));
        if borrowed || !at_own_path {
            out.insert(kind.slot_number(), p.to_string_lossy().replace('/', "\\"));
        }
    }
    for n in 1..=8u8 {
        if let Some(Value::String(s)) = v.settings.get(&format!("slot{n}")) {
            out.insert(n, s.clone());
        }
    }
    out
}

/// Filled slots whose texture has no output path (its owner was dropped as a duplicate, or its prefix is unusable),
/// as (slot number, source set; `None` for the set's own texture). The file would not be where the JSON points
/// (a borrowed slot, or PGPatcher's default `<prefix>_<suffix>.dds`), so Export blocks (unless `slotN` is set).
fn unresolved_slots(
    v: &SetView,
    views: &[SetView],
    paths: &BTreeMap<OutputKey, PathBuf>,
    m: &AppModel,
) -> Vec<(u8, Option<String>)> {
    let mut out = vec![];
    for (kind, fill) in &v.slots {
        let (item, from) = match fill {
            SlotFill::Borrowed { item, from } => (item, Some(from.clone())),
            SlotFill::Own { item } => (item, None),
            SlotFill::Empty => continue,
        };
        let n = kind.slot_number();
        if matches!(v.settings.get(&format!("slot{n}")), Some(Value::String(s)) if !s.is_empty()) {
            continue;
        }
        if fill_path(item, views, paths, m).is_none() {
            out.push((n, from));
        }
    }
    out
}

/// Why `name` cannot be the JSON file name (a single file name, without `.json`).
fn json_name_problem(name: &str) -> Option<String> {
    file_name_problem("JSON name", name)
}

/// Why `name` cannot be a single Windows file name (without its extension); `what` starts the reason.
pub(crate) fn file_name_problem(what: &str, name: &str) -> Option<String> {
    let bad = |why: &str| Some(format!("{what}: {why} ({name:?})"));
    if name.trim().is_empty() {
        return bad("empty");
    }
    if name.contains(['\\', '/']) {
        return bad("must be a single file name, not a path");
    }
    if name == ".." || name.contains("..") {
        return bad("must not contain '..'");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return bad("must not end in a dot or a space");
    }
    if check_rel_path(name, "JSON name").is_err() {
        return bad("contains a character Windows does not allow in names");
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        });
    if reserved {
        return bad("is a name Windows reserves for a device");
    }
    None
}

/// Every PBR set's JSON entry, in display order (CS only; empty otherwise).
pub fn json_entries(m: &AppModel) -> Vec<(SetView, EntryInput)> {
    if m.target != Target::Cs {
        return vec![];
    }
    let (_, paths) = engine_inputs(m);
    let views = set_views(m);
    views
        .iter()
        // An entry with no match could never apply, and the schema refuses one (F1 spec §3.2, DECISIONS V4).
        .filter(|v| {
            v.pbr && !matches_nothing(&v.settings, v.derived_match.as_deref().unwrap_or(""))
        })
        .map(|v| {
            let e = EntryInput {
                settings: crate::model::auto_flags::resolved_for(
                    v.pbr,
                    &v.slots,
                    &v.settings,
                    &crate::model::auto_flags::hint(m, v),
                ),
                slots: slots_of(v, &views, &paths, m),
                derived_match: v.derived_match.clone().unwrap_or_default(),
                section: false,
            };
            (v.clone(), e)
        })
        .collect()
}

/// `json_entries` less the sets whose entry the statics rule drops (design §5.1): what the written file holds.
pub fn written_json_entries(m: &AppModel) -> Vec<(SetView, EntryInput)> {
    json_entries(m)
        .into_iter()
        .filter(|(v, _)| m.pgp_entry_wanted(v))
        .collect()
}

const NEEDS: [(Feature, SlotKind, &str, &str); 3] = [
    (
        Feature::Emissive,
        SlotKind::Emissive,
        "emissive",
        "emissive texture (_g)",
    ),
    (
        Feature::Parallax,
        SlotKind::Height,
        "parallax",
        "height texture (_p)",
    ),
    (
        Feature::Subsurface,
        SlotKind::Subsurface,
        "subsurface",
        "subsurface texture (_s)",
    ),
];

fn has(v: &SetView, s: SlotKind) -> bool {
    !matches!(v.slots.get(&s), None | Some(SlotFill::Empty))
}

/// The "feature is on but its texture is missing" blocks of one PBR set; `has` says whether a slot is filled.
pub fn missing_texture_texts(
    id: &str,
    s: &Settings,
    has: impl Fn(SlotKind) -> bool,
) -> Vec<String> {
    let mut out = vec![];
    for (f, slot, name, what) in NEEDS {
        if feature_on(s, f) && blocked_by(s, f).is_none() && !has(slot) {
            out.push(format!("{id}: {name} is on but the set has no {what}"));
        }
    }
    // Foliage reads the same `_s` slot 8 as Standard (AG101).
    if s.get(crate::pgp::settings::FOLIAGE_KEY) == Some(&Value::Bool(true))
        && blocked_by(s, Feature::Subsurface).is_none()
        && !has(SlotKind::Subsurface)
    {
        out.push(format!(
            "{id}: subsurface foliage is on but the set has no subsurface texture (_s)"
        ));
    }
    if feature_on(s, Feature::Multilayer)
        && s.get("coat_normal") == Some(&Value::Bool(true))
        && !has(SlotKind::Coat)
    {
        out.push(format!(
            "{id}: coat normal is on but the set has no coat normal texture (_cnr)"
        ));
    }
    if feature_on(s, Feature::Fuzz)
        && s.get("fuzz.texture") == Some(&Value::Bool(true))
        && !has(SlotKind::Fuzz)
    {
        out.push(format!(
            "{id}: fuzz texture is on but the set has no fuzz texture (_f)"
        ));
    }
    out
}

/// What stops Export (spec §6.3). On CS the PBR sets are checked; slot clashes apply to every set. Cached with the
/// set views (audit 7, DECISIONS Note Y7).
pub fn export_blocks(m: &AppModel) -> Vec<ExportIssue> {
    let key = m.view_key();
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some(b) = &c.blocks {
                return (**b).clone();
            }
        }
    }
    // No borrow is held here: `build_blocks` reads the cached views.
    let b = Arc::new(build_blocks(m));
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = ViewCache {
            key: Some(key),
            ..Default::default()
        };
    }
    c.blocks = Some(b.clone());
    (*b).clone()
}

fn build_blocks(m: &AppModel) -> Vec<ExportIssue> {
    let mut out = vec![];
    for v in &set_views(m) {
        // Design §5.1: a set whose entry the statics rule drops writes no JSON, so nothing of it blocks.
        if m.target == Target::Cs && !m.pgp_entry_wanted(v) {
            continue;
        }
        if has(v, SlotKind::Coat) && has(v, SlotKind::Fuzz) {
            out.push(issue(v, format!("{}: _cnr and _f both use slot 7", v.id)));
        }
        if has(v, SlotKind::Emissive) && has(v, SlotKind::SkinSk) {
            out.push(issue(v, format!("{}: _g and _sk both use slot 3", v.id)));
        }
        if m.target != Target::Cs || !v.pbr {
            continue;
        }
        let s = &v.settings;
        for text in missing_texture_texts(&v.id, s, |k| has(v, k)) {
            out.push(issue(v, text));
        }
        for (f, _) in FEATURE_KEYS {
            if let Some(b) = feature_on(s, f).then(|| blocked_by(s, f)).flatten() {
                if f < b {
                    let text = format!(
                        "{}: {} and {} can't both be on",
                        v.id,
                        feature_key(f).trim_end_matches("_on"),
                        feature_key(b).trim_end_matches("_on")
                    );
                    // Note FC3 (P26): the row turns the second one off.
                    out.push(ExportIssue {
                        fix: Some(Fix::TurnFeatureOff(v.id.clone(), b)),
                        ..issue(v, text)
                    });
                }
            }
        }
        // Note FC3 (P13): every path error starts with its field's key; the row goes to that field when it has a box
        // to focus, else it keeps Go to set.
        let fix_path = |e: &str| {
            let field = e.split_once(':').map_or(e, |(f, _)| f);
            // `rename` and `match1`-`match8` have boxes in the Advanced group's Match block.
            crate::pgp::prefix::is_path_key(field)
                .then(|| Fix::FixPath(v.key.clone(), field.to_string()))
        };
        if let Some(e) = &v.prefix_error {
            out.push(ExportIssue {
                fix: fix_path(e),
                ..issue(v, format!("{}: {e}", v.id))
            });
        }
        for n in 1..=8u8 {
            if let Some(Value::String(p)) = s.get(&format!("slot{n}")) {
                if !p.is_empty() {
                    if let Err(e) = check_json_path(p, &format!("slot{n}")) {
                        // Slot paths have no focusable box (the JSON editor holds them): the row offers Go to set.
                        out.push(issue(v, format!("{}: {}", v.id, e.0)));
                    }
                }
            }
        }
    }
    out.extend(path_collisions(m));
    if m.target == Target::Cs {
        // A set whose entry the statics rule drops writes nothing, so it cannot block the JSON (design §5.1).
        let entries: Vec<EntryInput> = written_json_entries(m)
            .into_iter()
            .map(|(_, e)| e)
            .collect();
        if !entries.is_empty() {
            // A filled slot with no path is reported only when nothing above already explains it (a dropped
            // duplicate or an unusable prefix is already a block of its own).
            if out.is_empty() {
                let views = set_views(m);
                let (_, paths) = engine_inputs(m);
                for v in views.iter().filter(|v| v.pbr && m.pgp_entry_wanted(v)) {
                    for (n, from) in unresolved_slots(v, &views, &paths, m) {
                        let text = match from {
                            Some(from) => format!(
                                "{}: slot{n} borrows from {from}, which has no output path",
                                v.id
                            ),
                            None => format!("{}: slot{n}'s texture has no output path", v.id),
                        };
                        out.push(issue(v, text));
                    }
                }
            }
            if let Some(why) = json_name_problem(&json_name(m)) {
                out.push(general(why));
            }
            if let Err(errs) = validate(&document(&entries).to_value()) {
                out.push(general(format!(
                    "PGPatcher JSON does not validate: {}",
                    errs[0]
                )));
            }
        }
    }
    out
}

fn path_lower(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Two outputs on one path. Owned outputs are compared before `engine_inputs` drops the second, so the engine never
/// sees the collision (Review focus 2). An owned output is also compared with the mirrored path of every output that
/// has no explicit path: the engine does not re-check those (Task 10 review, Minor 8).
pub fn path_collisions(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    let views = set_views(m);
    let name = |k: &OutputKey| match k {
        OutputKey::File(f) => f.clone(),
        OutputKey::Packed { set, suffix } => format!("{set} ({suffix})"),
    };
    let key_of_set = |id: &str| {
        views
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.key.clone())
            .unwrap_or_default()
    };
    let candidates = output_candidates(m, &views);
    let mut seen: BTreeMap<String, (String, OutputKey)> = BTreeMap::new();
    let mut out = vec![];
    for (owner, k, p) in &candidates {
        match seen.get(&path_lower(p)) {
            Some((first_owner, first)) => out.push(ExportIssue {
                set_id: owner.clone(),
                set_key: key_of_set(owner),
                text: format!(
                    "{first_owner} and {owner} both write {} ({} and {})",
                    p.display(),
                    name(first),
                    name(k)
                ),
                fix: None,
            }),
            None => {
                seen.insert(path_lower(p), (owner.clone(), k.clone()));
            }
        }
    }
    // Mirrored outputs: items that will write a file but have no explicit path.
    let owned: Vec<&OutputKey> = candidates.iter().map(|(_, k, _)| k).collect();
    for it in &m.items {
        if it.job.is_none() || it.job_key.is_empty() {
            continue;
        }
        if item_output_key(&views, it).is_some_and(|k| owned.contains(&&k)) {
            continue;
        }
        if let Some((owner, k)) = seen.get(&it.job_key.replace('\\', "/").to_lowercase()) {
            out.push(ExportIssue {
                set_id: owner.clone(),
                set_key: key_of_set(owner),
                text: format!(
                    "{owner} writes {} ({}), the same file {} is written to (no path of its own)",
                    it.job_key,
                    name(k),
                    it.key
                ),
                fix: None,
            });
        }
    }
    out
}

/// What Review asks to acknowledge: first what re-encodes after an upgrade (FC7, project-wide), then the pitfalls
/// of every PBR set.
pub fn export_asks(m: &AppModel) -> Vec<ExportIssue> {
    let recode = m
        .recode
        .iter()
        .filter(|c| !matches!(c.why, engine::manifest::RecodeWhy::FirstExport { .. }))
        .map(|c| general(c.line()));
    recode.chain(set_asks(m)).collect()
}

/// The PBR sets' pitfalls. Cached like `export_blocks`, plus `evidence_rev` (the known texture paths and the mesh
/// tree flags it reads).
fn set_asks(m: &AppModel) -> Vec<ExportIssue> {
    let key = m.view_key();
    {
        let c = m.views.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some((rev, a)) = &c.asks {
                if *rev == m.evidence_rev {
                    return (**a).clone();
                }
            }
        }
    }
    // No borrow is held here: the build reads the cached views.
    let a = Arc::new(build_asks(m));
    let mut c = m.views.borrow_mut();
    if c.key.as_ref() != Some(&key) {
        *c = ViewCache {
            key: Some(key),
            ..Default::default()
        };
    }
    c.asks = Some((m.evidence_rev, a.clone()));
    (*a).clone()
}

fn build_asks(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    let known = m.known_textures(Some(0));
    set_views(m)
        .iter()
        .filter(|v| v.pbr)
        .flat_map(|v| {
            let derived = v.derived_match.as_deref().unwrap_or("");
            pitfalls(&v.settings, derived)
                .into_iter()
                .chain(subsurface_pitfalls(
                    &v.settings,
                    crate::model::auto_flags::hint(m, v).tree_anim,
                ))
                .chain(rename_hits_elsewhere(&v.settings, derived, &known))
                .map(move |w| issue(v, format!("{}: {w}", v.id)))
        })
        .collect()
}

/// What an export about to start writes once it has ended: the PGPatcher JSON and the vanilla copies.
#[derive(Debug, Clone)]
pub struct PendingJson {
    /// The folder the JSON's hash records are kept against: the PBR folder while a tick is on (Note CN, re-check
    /// A4), else the export folder, as `PendingCs.root` and `PendingCm.root`.
    pub root: PathBuf,
    pub path: PathBuf,
    /// Set id, the job keys its entry depends on, and the entry.
    pub entries: Vec<(String, Vec<String>, EntryInput)>,
    /// Borrowed-slot copies for vanilla: (owner output, borrower path, owner job key), paths absolute (P4).
    pub copies: Vec<(PathBuf, PathBuf, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    Written(String),
    /// The file was edited outside the GUI: ask, with this text ready to write.
    Overwrite(String),
    Nothing,
}

/// What the export about to start will write afterwards (computed when it starts, so edits made while it runs do
/// not change it).
pub fn prepare(m: &AppModel) -> Option<PendingJson> {
    let views = set_views(m);
    if m.target != Target::Cs {
        let mut copies = vec![];
        for v in &views {
            let Some(SlotFill::Own { item: d }) = v.slots.get(&SlotKind::Diffuse) else {
                continue;
            };
            let Some(dout) = m
                .item(d)
                .and_then(|i| i.job.as_ref())
                .map(|j| j.output.clone())
            else {
                continue;
            };
            let stem = dout
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for (kind, f) in &v.slots {
                let SlotFill::Borrowed { item, .. } = f else {
                    continue;
                };
                let Some((src, owner_key)) = m.item(item).and_then(|i| {
                    i.job
                        .as_ref()
                        .map(|j| (j.output.clone(), i.job_key.clone()))
                }) else {
                    continue;
                };
                let name = if kind.suffix().is_empty() {
                    format!("{stem}.dds")
                } else {
                    format!("{stem}_{}.dds", kind.suffix())
                };
                copies.push((src, dout.with_file_name(name), owner_key));
            }
        }
        return (!copies.is_empty()).then(|| PendingJson {
            root: m.output_root(),
            path: PathBuf::new(),
            entries: vec![],
            copies,
        });
    }
    let deps = |v: &SetView| -> Vec<String> {
        v.slots
            .values()
            .filter_map(|f| match f {
                SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => m
                    .item(item)
                    .map(|i| i.job_key.clone())
                    .filter(|j| !j.is_empty()),
                SlotFill::Empty => None,
            })
            .collect()
    };
    // Design §5.1: a set with a texture-set block gets its entry only for the statics that use its diffuse.
    let mut entries: Vec<_> = written_json_entries(m)
        .into_iter()
        .map(|(v, e)| (v.id.clone(), deps(&v), e))
        .collect();
    // GUI tweaks G4: the delete sections go last, so nothing after them changes the shapes they match.
    for (i, s) in m.sections().into_iter().enumerate() {
        if crate::model::sections::written(&s) {
            let e = EntryInput {
                settings: s,
                section: true,
                ..EntryInput::default()
            };
            entries.push((format!("delete section {}", i + 1), vec![], e));
        }
    }
    if entries.is_empty() {
        return None;
    }
    let root = m.output_root();
    Some(PendingJson {
        path: root
            .join("PBRNIFPatcher")
            .join(format!("{}.json", json_name(m))),
        entries,
        copies: vec![],
        root,
    })
}

fn exported(rows: &[Row], key: &str) -> Result<(), String> {
    match rows.iter().find(|r| r.job_key.eq_ignore_ascii_case(key)) {
        Some(Row {
            state: RowState::Done { .. } | RowState::UpToDate,
            ..
        }) => Ok(()),
        Some(r) => Err(r.state.label()),
        None => Err("not exported".into()),
    }
}

/// After a successful export, the vanilla borrowed-slot copies (P4, P22). A copy replaces only a file it wrote
/// before (its hash is in `shas`, keyed by `history::record_key`) or one with the same bytes; any other file is
/// kept with a note (Note BV3). `shas` gets the hash of each copy written.
pub fn copy_files(
    p: &PendingJson,
    rows: &[Row],
    shas: &mut std::collections::BTreeMap<String, String>,
    notes: &mut Vec<String>,
) {
    for (src, dst, owner_key) in &p.copies {
        if !owner_key.is_empty() {
            if let Err(why) = exported(rows, owner_key) {
                notes.push(format!("could not copy {} ({why})", src.display()));
                continue;
            }
        }
        let bytes = match std::fs::read(src) {
            Ok(b) => b,
            Err(e) => {
                notes.push(format!(
                    "could not copy {} ({e}; not exported?)",
                    src.display()
                ));
                continue;
            }
        };
        let key = super::history::record_key(dst);
        if let Err(k) =
            engine::dds::may_replace(dst, &bytes, recorded(shas.get(&key).map(String::as_str)))
        {
            notes.push(k.note(dst));
            continue;
        }
        match engine::dds::write_atomic(dst, &bytes) {
            Ok(()) => {
                use sha2::{Digest, Sha256};
                shas.insert(key, hex::encode(Sha256::digest(&bytes)));
                notes.push(format!("copied {} → {}", src.display(), dst.display()));
            }
            Err(e) => notes.push(format!("could not copy {} ({e})", src.display())),
        }
    }
}

/// After a successful export: the JSON of the sets that fully exported (spec §6.1, §6.4).
pub fn finish(
    p: &PendingJson,
    rows: &[Row],
    last_sha: Option<&str>,
    notes: &mut Vec<String>,
) -> Finish {
    if p.entries.is_empty() {
        return Finish::Nothing;
    }
    let mut kept = vec![];
    for (id, deps, e) in &p.entries {
        match deps.iter().map(|k| exported(rows, k)).find_map(Result::err) {
            None => kept.push(e.clone()),
            Some(why) => notes.push(format!("not in JSON: {id} ({why})")),
        }
    }
    // DECISIONS AF1: delete sections alone would strip vanilla shapes with no PBR replacement; keep the old JSON.
    if p.entries.iter().all(|(_, _, e)| e.section) {
        notes.push(
            "PGPatcher JSON not written: there is no PBR set, and delete sections alone are not written".into(),
        );
        return Finish::Nothing;
    }
    if kept.iter().all(|e| e.section) {
        notes.push(
            "PGPatcher JSON not written: no PBR set exported completely (the previous JSON is kept; delete sections alone would remove shapes with no PBR replacement)"
                .into(),
        );
        return Finish::Nothing;
    }
    let doc = document(&kept);
    if let Err(errs) = validate(&doc.to_value()) {
        notes.push(format!(
            "PGPatcher JSON not written: it does not validate ({})",
            errs[0]
        ));
        return Finish::Nothing;
    }
    let text = to_text(&doc);
    // Note BV3: a file edited outside Tessalith asks (P21); one that cannot be read is kept.
    match engine::dds::may_replace(&p.path, text.as_bytes(), recorded(last_sha)) {
        Ok(()) => {}
        Err(engine::dds::Kept::Foreign) => return Finish::Overwrite(text),
        Err(k) => {
            notes.push(format!("PGPatcher JSON not written: {}", k.note(&p.path)));
            return Finish::Nothing;
        }
    }
    match engine::dds::write_atomic(&p.path, text.as_bytes()) {
        Ok(()) => {
            notes.push(format!(
                "PGPatcher JSON written: {} ({} entries)",
                p.path.display(),
                kept.len()
            ));
            sha256_of(&p.path).map_or(Finish::Nothing, Finish::Written)
        }
        Err(e) => {
            notes.push(format!("PGPatcher JSON not written: {e}"));
            Finish::Nothing
        }
    }
}

/// Write `text` over an existing (hand-edited) JSON: Review's answer "Overwrite it" (Note FC5).
pub fn overwrite(path: &std::path::Path, text: &str, notes: &mut Vec<String>) -> Option<String> {
    match engine::dds::write_atomic(path, text.as_bytes()) {
        Ok(()) => {
            notes.push(format!("PGPatcher JSON written: {}", path.display()));
            sha256_of(path)
        }
        Err(e) => {
            notes.push(format!("PGPatcher JSON not written: {e}"));
            None
        }
    }
}

/// The note for an export that did not end cleanly: nothing is written then, neither the JSON nor the vanilla
/// copies (audit 08f-3).
pub fn unfinished_note(p: &PendingJson, failed: bool) -> Option<String> {
    let what = if !p.entries.is_empty() {
        "PGPatcher JSON not written"
    } else if !p.copies.is_empty() {
        "borrowed-slot copies not made"
    } else {
        return None;
    };
    let why = if failed { "failed" } else { "was cancelled" };
    Some(format!("{what}: the export {why}"))
}

/// What the PGPatcher JSON editor shows (DECISIONS AG106): the export blocks (errors) and the pitfalls the export
/// asks about (warnings), each on its entry or in the general box. An Advanced skin set (AG90) or any set that is
/// not a TruePBR set has no entry and no line here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JsonIssues {
    /// `(set id, text)` for an entry in the editor.
    pub errors: Vec<(String, String)>,
    pub warnings: Vec<(String, String)>,
    /// Belong to no entry: project-wide blocks, and a TruePBR set that has no entry.
    pub general_errors: Vec<String>,
    pub general_warnings: Vec<String>,
}

impl JsonIssues {
    pub fn error_count(&self) -> usize {
        self.errors.len() + self.general_errors.len()
    }
    pub fn warning_count(&self) -> usize {
        self.warnings.len() + self.general_warnings.len()
    }
}

/// Sorts the blocks and the pitfalls onto `entry_ids` (the sets that have an entry), by set id.
pub fn json_issues(m: &AppModel, entry_ids: &[&str]) -> JsonIssues {
    let views = set_views(m);
    let mut out = JsonIssues::default();
    let mut place = |i: ExportIssue, error: bool| {
        let owned = entry_ids.iter().any(|e| same_id(e, &i.set_id));
        if owned {
            // "<set id>: <message>" shows as the message, under the entry that says whose it is.
            let text = i.text.strip_prefix(&format!("{}: ", i.set_id));
            let text = text.unwrap_or(&i.text).to_string();
            let to = if error {
                &mut out.errors
            } else {
                &mut out.warnings
            };
            to.push((i.set_id, text));
        } else if i.set_id.is_empty() || views.iter().any(|v| v.pbr && same_id(&v.id, &i.set_id)) {
            let to = if error {
                &mut out.general_errors
            } else {
                &mut out.general_warnings
            };
            to.push(i.text);
        }
    };
    for i in export_blocks(m) {
        place(i, true);
    }
    // FC7's re-encode rows are not the JSON's: the sets' pitfalls only.
    for i in set_asks(m) {
        place(i, false);
    }
    out
}
