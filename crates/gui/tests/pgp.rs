use sdc_gui::pgp::schema::*;
use sdc_gui::pgp::settings::*;
use serde_json::{json, Value};

/// The entry keys in the schema file's own order. `serde_json::Map` is a `BTreeMap` here (no `preserve_order`), so
/// it would hand them back alphabetically; this reads the keys through a visitor instead.
fn entry_properties() -> Vec<String> {
    use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
    struct Keys(Vec<String>);
    impl<'de> Visitor<'de> for Keys {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object")
        }
        fn visit_map<A: MapAccess<'de>>(mut self, mut m: A) -> Result<Self::Value, A::Error> {
            while let Some((k, _)) = m.next_entry::<String, IgnoredAny>()? {
                self.0.push(k);
            }
            Ok(self.0)
        }
    }
    let raw = SCHEMA_JSON;
    // Find the `properties` object of `definitions.entry` in the raw text and read its keys in order.
    let entry = raw.find("\"entry\"").expect("entry definition");
    let at = entry
        + raw[entry..]
            .find("\"properties\"")
            .expect("entry properties");
    let start = at + raw[at..].find('{').unwrap();
    let mut de = serde_json::Deserializer::from_str(&raw[start..]);
    de.deserialize_map(Keys(Vec::new())).unwrap()
}

#[test]
fn every_schema_key_is_handled_exactly_once() {
    let mut handled: Vec<String> = KEYS
        .iter()
        .filter(|k| !k.key.contains('.'))
        .map(|k| k.key.to_string())
        .collect();
    handled.extend(REMOVED.iter().map(|s| s.to_string()));
    handled.extend(NOT_EXPOSED.iter().map(|s| s.to_string()));
    let mut props = entry_properties();
    props.sort();
    handled.sort();
    assert_eq!(
        handled, props,
        "a schema key without a rule (or a rule for a key the schema lacks)"
    );
    // Nested keys of glint and fuzz.
    let s: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
    for obj in ["glint", "fuzz"] {
        for k in s["definitions"]["entry"]["properties"][obj]["properties"]
            .as_object()
            .unwrap()
            .keys()
        {
            assert!(spec(&format!("{obj}.{k}")).is_some(), "{obj}.{k}");
        }
    }
}

#[test]
fn keys_follow_schema_order() {
    let props = entry_properties();
    let order: Vec<usize> = KEYS
        .iter()
        .filter(|k| !k.key.contains('.'))
        .map(|k| props.iter().position(|p| p == k.key).unwrap())
        .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "KEYS must list keys in schema order: {order:?}"
    );
}

#[test]
fn kinds_match_the_owner_rules() {
    for k in ["emissive", "parallax", "subsurface"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::AlwaysBool, "{k}");
    }
    for k in ["hair", "multilayer"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::OnlyTrue, "{k}");
    }
    for k in ["lock_diffuse", "lock_cnr", "vertex_colors", "zbuffer_write"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::Tri, "{k}");
    }
    assert_eq!(spec("emissive_color").unwrap().kind, KeyKind::Rgba);
    assert!(spec("delete").is_none() && spec("pbr").is_none());
}

fn s(pairs: &[(&str, Value)]) -> Settings {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn mutual_exclusion_follows_the_owner_table() {
    use Feature::*;
    let hair = s(&[("hair", json!(true))]);
    for f in [Multilayer, Glint, Fuzz, Emissive, Parallax, Subsurface] {
        assert_eq!(blocked_by(&hair, f), Some(Hair), "{f:?}");
    }
    let ml = s(&[("multilayer", json!(true))]);
    for f in [Subsurface, Glint, Fuzz, Hair] {
        assert_eq!(blocked_by(&ml, f), Some(Multilayer), "{f:?}");
    }
    assert_eq!(blocked_by(&ml, Emissive), None);
    assert_eq!(blocked_by(&ml, Parallax), None);
    let glint = s(&[("glint", json!(true))]);
    assert_eq!(blocked_by(&glint, Fuzz), Some(Glint));
    assert_eq!(blocked_by(&glint, Multilayer), Some(Glint));
    let ss = s(&[("subsurface", json!(true))]);
    assert_eq!(blocked_by(&ss, Multilayer), Some(Subsurface));
    assert_eq!(blocked_by(&ss, Emissive), None);
    // A feature never blocks itself, and off features block nothing.
    assert_eq!(blocked_by(&hair, Hair), None);
    assert_eq!(blocked_by(&s(&[("hair", json!(false))]), Emissive), None);
}

#[test]
fn effective_overlays_own_values_on_defaults() {
    let d = s(&[("specular_level", json!(0.04)), ("parallax", json!(true))]);
    let own = s(&[("parallax", json!(false))]);
    let e = effective(&d, &own);
    assert_eq!(e["specular_level"], json!(0.04));
    assert_eq!(e["parallax"], json!(false));
}
