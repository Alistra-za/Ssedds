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

use sdc_gui::pgp::prefix::*;

#[test]
fn derived_match_drops_textures_pbr_and_the_extension() {
    assert_eq!(
        derive_match_diffuse("landscape", "rock01"),
        r"landscape\rock01"
    );
    assert_eq!(
        derive_match_diffuse("textures/landscape", "rock01"),
        r"landscape\rock01"
    );
    assert_eq!(
        derive_match_diffuse("Textures/PBR/Landscape", "Rock01"),
        r"Landscape\Rock01"
    );
    assert_eq!(derive_match_diffuse("", "rock01"), "rock01");
}

#[test]
fn diffuse_mode_prefixes_and_renames() {
    let p = |pairs: &[(&str, Value)], derived: &str| prefix_of(&s(pairs), derived).unwrap();
    assert_eq!(
        p(&[], r"landscape\rock01"),
        r"textures\pbr\landscape\rock01"
    );
    assert_eq!(
        p(
            &[("match_diffuse", json!(r"textures\landscape\rock02.dds"))],
            "x"
        ),
        r"textures\pbr\landscape\rock02"
    );
    assert_eq!(
        p(&[("match_diffuse", json!(r"pbr\landscape\rock02"))], "x"),
        r"textures\pbr\landscape\rock02",
        "never doubled"
    );
    assert_eq!(
        p(
            &[("rename", json!(r"landscape\boulder"))],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\boulder"
    );
    assert_eq!(
        p(&[("match_normal", json!(r"landscape\rock01_n.dds"))], "x"),
        r"textures\pbr\landscape\rock01"
    );
    assert_eq!(
        p(&[("path_contains", json!("rock"))], r"landscape\rock01"),
        r"textures\pbr\landscape\rock01"
    );
}

#[test]
fn slot_modes_strip_the_first_suffix_of_their_own_list() {
    let p = |key: &str, v: &str| prefix_of(&s(&[(key, json!(v))]), "unused").unwrap();
    assert_eq!(
        p("match2", r"architecture\whiterun\wrplank01_n.dds"),
        r"textures\pbr\architecture\whiterun\wrplank01"
    );
    assert_eq!(
        p("match1", r"actors\facemask.dds"),
        r"textures\pbr\actors\face",
        "match1 strips a bare 'mask'"
    );
    assert_eq!(
        p("match1", r"x\rock_n.dds"),
        r"textures\pbr\x\rock_n",
        "a suffix on the wrong slot stays"
    );
    assert_eq!(
        p("match8", r"x\leaf_s.dds"),
        r"textures\pbr\x\leaf_s",
        "match8 does not strip _s"
    );
    assert_eq!(p("match7", r"x\leaf_s.dds"), r"textures\pbr\x\leaf");
    assert_eq!(p("match6", r"x\iron_m.dds"), r"textures\pbr\x\iron");
    // rename replaces the whole prefix in slot modes.
    let r = prefix_of(
        &s(&[
            ("match2", json!(r"a\b_n.dds")),
            ("rename", json!(r"textures\pbr\c\d")),
        ]),
        "x",
    )
    .unwrap();
    assert_eq!(r, r"textures\pbr\c\d");
}

#[test]
fn bad_paths_block() {
    for bad in [r"..\up", r"C:\abs", r"\lead", "a|b", r"a\..\b"] {
        assert!(
            prefix_of(&s(&[("match_diffuse", json!(bad))]), "x").is_err(),
            "{bad:?}"
        );
        assert!(
            prefix_of(&s(&[("rename", json!(bad))]), "x").is_err(),
            "{bad:?}"
        );
        assert!(check_rel_path(bad, "slot3").is_err(), "{bad:?}");
    }
    assert!(check_rel_path("", "slot3").is_err());
    // An empty match_diffuse falls back to the derived match; an empty derived match blocks.
    assert!(prefix_of(&s(&[]), "").is_err());
    assert!(check_rel_path(r"landscape\white.dds", "slot3").is_ok());
}

#[test]
fn pitfalls_are_reported_not_fixed() {
    let w = pitfalls(&s(&[("match8", json!(r"x\leaf_s.dds"))]));
    assert!(w.iter().any(|m| m.contains("match8")), "{w:?}");
    let w = pitfalls(&s(&[
        ("match2", json!(r"a\b_n.dds")),
        ("rename", json!(r"c\d")),
    ]));
    assert!(w.iter().any(|m| m.contains("full path")), "{w:?}");
    assert!(pitfalls(&s(&[("match_diffuse", json!("a"))])).is_empty());
}

#[test]
fn output_paths_use_the_canonical_suffix() {
    assert_eq!(
        output_rel(r"textures\pbr\landscape\rock01", ""),
        std::path::PathBuf::from("textures/pbr/landscape/rock01.dds")
    );
    assert_eq!(
        output_rel(r"textures\pbr\landscape\rock01", "rmaos"),
        std::path::PathBuf::from("textures/pbr/landscape/rock01_rmaos.dds")
    );
}
