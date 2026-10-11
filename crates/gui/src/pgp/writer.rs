//! The PGPatcher JSON document (M3b spec §5.2–5.3, §5.6): `{ "$schema", "default", "entries" }`, keys in schema
//! order. serde_json's map sorts keys, so the document is an ordered tree (`Node`) printed by `to_text`.
use super::prefix::{match_mode, MatchMode};
use super::schema::{Group, KeyKind, KEYS, SCHEMA_URL};
use super::settings::{effective, feature_on, Feature, Settings, GLINT_DEFAULTS};
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
    pub derived_match: String,
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

/// Whether feature `f` is written as on. Two exclusive features can both be on only through a default plus an own
/// value (the UI greys the second); then exactly one is written, by precedence: hair, then multilayer, then glint,
/// then fuzz; emissive and parallax yield to hair, subsurface to hair and multilayer.
fn live(eff: &Settings, f: Feature) -> bool {
    use Feature::*;
    let on = |g| feature_on(eff, g);
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

/// The entries of one object: `own` written under the rules; `eff` decides features; `derived` is the derived match
/// (`None` for the default block); `slots` final slot paths.
fn object(
    defaults: &Settings,
    own: &Settings,
    eff: &Settings,
    derived: Option<&str>,
    slots: &BTreeMap<u8, String>,
) -> Vec<(String, Node)> {
    let mut out = vec![];
    let mode = match_mode(eff);
    for k in KEYS.iter().filter(|k| !k.key.contains('.')) {
        if let Some(f) = group_feature(k.group) {
            if !live(eff, f) {
                continue;
            }
        }
        let v = own.get(k.key);
        let node = match k.kind {
            KeyKind::AlwaysBool => {
                if derived.is_none() && v.is_none() {
                    None // the default block writes only what it sets
                } else {
                    let f = feature_of_key(k.key).expect("always-written keys are features");
                    Some(Node::Scalar(json!(live(eff, f))))
                }
            }
            KeyKind::OnlyTrue => {
                let on = v.and_then(Value::as_bool) == Some(true);
                let ok = feature_of_key(k.key).is_none_or(|f| live(eff, f));
                // PGPatcher copies the default block into entries, so an entry can only switch a default-on key
                // off by writing `false` itself.
                let switched_off = v.and_then(Value::as_bool) == Some(false)
                    && defaults.get(k.key).and_then(Value::as_bool) == Some(true);
                if switched_off {
                    Some(Node::Scalar(json!(false)))
                } else {
                    (on && ok).then_some(Node::Scalar(json!(true)))
                }
            }
            KeyKind::Object => {
                let f = feature_of_key(k.key).expect("objects are features");
                if !live(eff, f) || (derived.is_none() && v.is_none()) {
                    None
                } else {
                    Some(Node::Obj(sub_object(k.key, own, eff)))
                }
            }
            KeyKind::Tri | KeyKind::Number => {
                v.filter(|v| !v.is_null()).map(|v| Node::Scalar(v.clone()))
            }
            KeyKind::Text => {
                if k.key == "match_diffuse" {
                    match (derived, mode) {
                        (Some(d), MatchMode::Diffuse) => Some(Node::Scalar(json!(v
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .unwrap_or(d)))),
                        _ => None,
                    }
                } else if derived.is_none() && k.group == Group::Match {
                    None // match fields are per set only
                } else {
                    v.and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(|s| Node::Scalar(json!(s)))
                }
            }
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

fn sub_object(obj: &str, own: &Settings, eff: &Settings) -> Vec<(String, Node)> {
    let mut out = vec![];
    for k in KEYS
        .iter()
        .filter(|k| k.key.starts_with(&format!("{obj}.")))
    {
        let name = &k.key[obj.len() + 1..];
        let v = own
            .get(k.key)
            .or_else(|| eff.get(k.key))
            .filter(|v| !v.is_null());
        let node = match k.kind {
            KeyKind::Number => v
                .cloned()
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

pub fn entry_node(defaults: &Settings, e: &EntryInput) -> Node {
    let eff = effective(defaults, &e.settings);
    Node::Obj(object(
        defaults,
        &e.settings,
        &eff,
        Some(&e.derived_match),
        &e.slots,
    ))
}

pub fn document(defaults: &Settings, entries: &[EntryInput]) -> Node {
    let default = Node::Obj(object(
        &Settings::new(),
        defaults,
        defaults,
        None,
        &BTreeMap::new(),
    ));
    Node::Obj(vec![
        ("$schema".into(), Node::Scalar(json!(SCHEMA_URL))),
        ("default".into(), default),
        (
            "entries".into(),
            Node::Arr(entries.iter().map(|e| entry_node(defaults, e)).collect()),
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
