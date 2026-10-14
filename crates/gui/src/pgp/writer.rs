//! The PGPatcher JSON document (M3b spec §5.2–5.3, §5.6): `{ "$schema", "default", "entries" }`, keys in schema
//! order. serde_json's map sorts keys, so the document is an ordered tree (`Node`) printed by `to_text`.
use super::prefix::{active_matches, json_rename, match_value};
use super::schema::{Group, KeyKind, NumRange, KEYS, SCHEMA_URL};
use super::settings::{feature_on, Feature, Settings, GLINT_DEFAULTS};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar(Value),
    Obj(Vec<(String, Node)>),
    Arr(Vec<Node>),
}

impl Node {
    pub fn to_value(&self) -> Value {
        match self {
            Node::Scalar(v) => v.clone(),
            Node::Arr(a) => Value::Array(a.iter().map(Node::to_value).collect()),
            Node::Obj(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), v.to_value())).collect())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntryInput {
    /// The set's own settings (not merged with the defaults).
    pub settings: Settings,
    /// Final slot paths (commands and borrowed slots, already merged by the caller): slot → path, `""` clears.
    pub slots: BTreeMap<u8, String>,
    /// The derived diffuse path: `match1` while the set has no `match1` key (DECISIONS V2).
    pub derived_match: String,
    /// GUI tweaks G4: a delete section (no set): only its match keys, `nif_filter` and `delete` are written.
    pub section: bool,
}

fn colour(v: &Value, alpha: bool) -> Option<Node> {
    let mut out: Vec<Node> = v
        .as_array()?
        .iter()
        .take(3)
        .map(|c| {
            let x = c.as_f64().unwrap_or(0.0) / 255.0;
            Node::Scalar(json!((x * 10_000.0).round() / 10_000.0))
        })
        .collect();
    if out.len() != 3 {
        return None;
    }
    if alpha {
        out.push(Node::Scalar(json!(1.0)));
    }
    Some(Node::Arr(out))
}

/// `v` inside `range` (F1 spec §2): a hand-edited value outside it is written clamped. A value inside it, or one that
/// is not a number (the validator reports that), is written as it is.
fn clamped(v: &Value, range: NumRange) -> Value {
    match v.as_f64() {
        // A whole-number key is always written as an integer (the schema says integer).
        Some(x) if matches!(range, NumRange::Int(_)) => json!(range.clamp(x).round() as u64),
        Some(x) if range.clamp(x) != x => json!(range.clamp(x)),
        _ => v.clone(),
    }
}

fn group_feature(g: Group) -> Option<Feature> {
    Some(match g {
        Group::Emissive => Feature::Emissive,
        Group::Parallax => Feature::Parallax,
        Group::Subsurface => Feature::Subsurface,
        Group::Multilayer => Feature::Multilayer,
        Group::Glint => Feature::Glint,
        Group::Fuzz => Feature::Fuzz,
        _ => return None,
    })
}

fn feature_of_key(key: &str) -> Option<Feature> {
    super::settings::FEATURE_KEYS
        .iter()
        .find(|(_, k)| *k == key)
        .map(|(f, _)| *f)
}

/// The feature an `Object` key (`glint`, `fuzz`) belongs to: the object is written only while that feature is on.
fn feature_of_object(key: &str) -> Option<Feature> {
    match key {
        "glint" => Some(Feature::Glint),
        "fuzz" => Some(Feature::Fuzz),
        _ => None,
    }
}

/// Whether feature `f` is written as on. Two exclusive features can both be on only through hand-edited settings
/// (the UI greys the second); then exactly one is written, by precedence: hair, then multilayer, then glint, then
/// fuzz; emissive and parallax yield to hair, subsurface to hair and multilayer.
fn live(own: &Settings, f: Feature) -> bool {
    use Feature::*;
    let on = |g| feature_on(own, g);
    let hair = on(Hair);
    let ml = on(Multilayer) && !hair;
    let glint = on(Glint) && !hair && !ml;
    match f {
        Hair => hair,
        Multilayer => ml,
        Glint => glint,
        Fuzz => on(Fuzz) && !hair && !ml && !glint,
        Emissive => on(Emissive) && !hair,
        Parallax => on(Parallax) && !hair,
        Subsurface => on(Subsurface) && !hair && !ml,
    }
}

/// The flags of the fixed `default` block (all `false`). An entry writes one only as `true`.
pub const FIXED_FLAGS: [&str; 3] = ["emissive", "parallax", "subsurface"];
/// The numbers of the fixed `default` block. An entry omits one that is unset or equal to this value.
pub const FIXED_NUMBERS: [(&str, f64); 3] = [
    ("roughness_scale", 1.0),
    ("specular_level", 0.04),
    ("displacement_scale", 1.0),
];

/// The fixed `default` block (owner, 2026-10-10): six keys, in schema (KEYS) order. Users cannot change it.
fn fixed_default_block() -> Vec<(String, Node)> {
    KEYS.iter()
        .filter_map(|k| {
            if FIXED_FLAGS.contains(&k.key) {
                Some((k.key.to_string(), Node::Scalar(json!(false))))
            } else {
                FIXED_NUMBERS
                    .iter()
                    .find(|(n, _)| *n == k.key)
                    .map(|(n, x)| (n.to_string(), Node::Scalar(json!(x))))
            }
        })
        .collect()
}

/// The entry for one set: only the set's own settings, under the per-kind rules, in schema order.
fn object(own: &Settings, derived: &str, slots: &BTreeMap<u8, String>) -> Vec<(String, Node)> {
    let mut out = vec![];
    let matches = active_matches(own, derived);
    for k in KEYS.iter().filter(|k| !k.key.contains('.')) {
        if let Some(f) = group_feature(k.group) {
            if !live(own, f) {
                continue;
            }
        }
        let v = own.get(k.key).filter(|v| !v.is_null());
        let node = match k.kind {
            KeyKind::OnlyTrue => {
                let on = v.and_then(Value::as_bool) == Some(true);
                let ok = feature_of_key(k.key).is_none_or(|f| live(own, f));
                (on && ok).then_some(Node::Scalar(json!(true)))
            }
            KeyKind::Object => {
                let f = feature_of_object(k.key).expect("objects are features");
                live(own, f).then(|| Node::Obj(sub_object(k.key, own)))
            }
            KeyKind::Number(range) => {
                let fixed = FIXED_NUMBERS.iter().find(|(n, _)| *n == k.key);
                v.map(|v| clamped(v, range))
                    .filter(|v| fixed.is_none_or(|(_, x)| v.as_f64() != Some(*x)))
                    .map(Node::Scalar)
            }
            KeyKind::Tri => v.map(|v| Node::Scalar(v.clone())),
            KeyKind::Text => match k
                .key
                .strip_prefix("match")
                .and_then(|n| n.parse::<u8>().ok())
            {
                // matchN: the active slot matches only, in the JSON form (DECISIONS V2).
                Some(n) => matches
                    .iter()
                    .find(|(m, _)| *m == n)
                    .map(|(_, v)| Node::Scalar(json!(match_value(v)))),
                // The typed file name becomes the renamed path, with the `match_diffuse` PGPatcher renames from
                // (owner, 2026-10-14; DECISIONS V3, AA8). The match slots stay the entry's matches.
                None if k.key == "rename" => json_rename(own, derived).map(|(diffuse, r)| {
                    out.push(("match_diffuse".to_string(), Node::Scalar(json!(diffuse))));
                    Node::Scalar(json!(r))
                }),
                None => v
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| Node::Scalar(json!(s))),
            },
            KeyKind::Rgb => v.and_then(|v| colour(v, false)),
            KeyKind::Rgba => v.and_then(|v| colour(v, true)),
            KeyKind::Slot => {
                let n: u8 = k.key[4..].parse().expect("slotN");
                slots.get(&n).map(|p| Node::Scalar(json!(p)))
            }
        };
        if let Some(n) = node {
            out.push((k.key.to_string(), n));
        }
    }
    out
}

fn sub_object(obj: &str, own: &Settings) -> Vec<(String, Node)> {
    let mut out = vec![];
    for k in KEYS
        .iter()
        .filter(|k| k.key.starts_with(&format!("{obj}.")))
    {
        let name = &k.key[obj.len() + 1..];
        let v = own.get(k.key).filter(|v| !v.is_null());
        let node = match k.kind {
            KeyKind::Number(range) => v
                .map(|v| clamped(v, range))
                .or_else(|| {
                    GLINT_DEFAULTS
                        .iter()
                        .find(|(d, _)| *d == k.key)
                        .map(|(_, x)| json!(x))
                })
                .map(Node::Scalar),
            KeyKind::OnlyTrue => {
                (v.and_then(Value::as_bool) == Some(true)).then_some(Node::Scalar(json!(true)))
            }
            KeyKind::Rgb => v.and_then(|v| colour(v, false)),
            _ => None,
        };
        if let Some(n) = node {
            out.push((name.to_string(), n));
        }
    }
    out
}

pub fn entry_node(e: &EntryInput) -> Node {
    if e.section {
        return Node::Obj(section_object(&e.settings));
    }
    Node::Obj(object(&e.settings, &e.derived_match, &e.slots))
}

/// A delete section (GUI tweaks G4): its exact matches (in the JSON form), `path_contains`, `nif_filter`, then
/// `delete: true` (always: a section exists to delete shapes).
fn section_object(own: &Settings) -> Vec<(String, Node)> {
    let mut out = vec![];
    for key in crate::model::sections::SECTION_KEYS {
        let Some(v) = own.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let node = match key {
            "delete" => continue,
            k if k.starts_with("match") => v
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Node::Scalar(json!(match_value(&s.replace('/', "\\"))))),
            _ => v
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Node::Scalar(json!(s.replace('/', "\\")))),
        };
        if let Some(n) = node {
            out.push((key.to_string(), n));
        }
    }
    out.push(("delete".to_string(), Node::Scalar(json!(true))));
    out
}

pub fn document(entries: &[EntryInput]) -> Node {
    Node::Obj(vec![
        ("$schema".into(), Node::Scalar(json!(SCHEMA_URL))),
        ("default".into(), Node::Obj(fixed_default_block())),
        (
            "entries".into(),
            Node::Arr(entries.iter().map(entry_node).collect()),
        ),
    ])
}

/// Two-space indented JSON, `\n` line endings, a final newline. Scalars print as serde_json prints them.
pub fn to_text(doc: &Node) -> String {
    fn go(n: &Node, ind: usize, out: &mut String) {
        let pad = |i: usize| "  ".repeat(i);
        match n {
            Node::Scalar(v) => out.push_str(&serde_json::to_string(v).expect("scalars serialise")),
            Node::Arr(a) if a.iter().all(|x| matches!(x, Node::Scalar(_))) => {
                // Short scalar arrays (colours) on one line.
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    go(x, ind, out);
                }
                out.push(']');
            }
            Node::Arr(a) => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(ind + 1));
                    go(x, ind + 1, out);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push(']');
            }
            Node::Obj(o) if o.is_empty() => out.push_str("{}"),
            Node::Obj(o) => {
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    out.push_str(&pad(ind + 1));
                    out.push_str(&serde_json::to_string(k).expect("keys serialise"));
                    out.push_str(": ");
                    go(v, ind + 1, out);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push('}');
            }
        }
    }
    let mut s = String::new();
    go(doc, 0, &mut s);
    s.push('\n');
    s
}
