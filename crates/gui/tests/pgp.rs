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

use sdc_gui::pgp::writer::*;
use std::collections::BTreeMap;

fn entry(pairs: &[(&str, Value)], derived: &str) -> EntryInput {
    EntryInput {
        settings: s(pairs),
        slots: BTreeMap::new(),
        derived_match: derived.into(),
    }
}

fn written(e: &EntryInput, defaults: &Settings) -> Value {
    document(defaults, std::slice::from_ref(e)).to_value()["entries"][0].clone()
}

#[test]
fn a_plain_entry_writes_the_match_and_the_three_always_written_features() {
    let v = written(&entry(&[], r"landscape\rock01"), &Settings::new());
    assert_eq!(
        v,
        json!({ "match_diffuse": r"landscape\rock01", "emissive": false, "parallax": false, "subsurface": false })
    );
}

#[test]
fn defaults_feed_always_written_features_but_are_not_copied() {
    let d = s(&[("parallax", json!(true)), ("specular_level", json!(0.04))]);
    let v = written(&entry(&[], "x"), &d);
    assert_eq!(v["parallax"], json!(true));
    assert!(
        v.get("specular_level").is_none(),
        "the default block carries it"
    );
    let doc = document(&d, &[entry(&[], "x")]).to_value();
    assert_eq!(
        doc["default"],
        json!({ "parallax": true, "specular_level": 0.04 })
    );
    assert_eq!(doc["$schema"], json!(sdc_gui::pgp::schema::SCHEMA_URL));
}

#[test]
fn colours_convert_and_emissive_gets_alpha_one() {
    let v = written(
        &entry(
            &[
                ("emissive", json!(true)),
                ("emissive_color", json!([255, 128, 0])),
                ("emissive_scale", json!(2.0)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert_eq!(v["emissive_color"], json!([1.0, 0.502, 0.0, 1.0]));
    assert_eq!(v["emissive_scale"], json!(2.0));
}

#[test]
fn dependent_keys_need_their_feature() {
    let v = written(
        &entry(
            &[
                ("emissive_scale", json!(2.0)),
                ("displacement_scale", json!(0.4)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert!(v.get("emissive_scale").is_none() && v.get("displacement_scale").is_none());
}

#[test]
fn blocked_features_are_not_written() {
    let v = written(
        &entry(
            &[
                ("hair", json!(true)),
                ("parallax", json!(true)),
                ("multilayer", json!(true)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert_eq!(v["hair"], json!(true));
    assert_eq!(v["parallax"], json!(false));
    assert!(v.get("multilayer").is_none());
}

#[test]
fn glint_is_filled_from_its_defaults_and_fuzz_writes_what_is_set() {
    let v = written(
        &entry(
            &[
                ("glint", json!(true)),
                ("glint.microfacet_roughness", json!(0.02)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert_eq!(
        v["glint"],
        json!({ "screen_space_scale": 1.5, "log_microfacet_density": 40.0, "microfacet_roughness": 0.02, "density_randomization": 2.0 })
    );
    let v = written(
        &entry(
            &[
                ("fuzz", json!(true)),
                ("fuzz.texture", json!(true)),
                ("fuzz.color", json!([0, 0, 0])),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert_eq!(
        v["fuzz"],
        json!({ "texture": true, "color": [0.0, 0.0, 0.0] })
    );
}

#[test]
fn other_match_modes_omit_match_diffuse_and_tri_state_writes_only_when_set() {
    let v = written(
        &entry(
            &[
                ("match2", json!(r"a\b_n.dds")),
                ("lock_rmaos", json!(false)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert!(v.get("match_diffuse").is_none());
    assert_eq!(v["match2"], json!(r"a\b_n.dds"));
    assert_eq!(v["lock_rmaos"], json!(false));
    assert!(v.get("lock_diffuse").is_none());
}

#[test]
fn slots_and_key_order() {
    let mut e = entry(&[("specular_level", json!(0.04))], "x");
    e.slots.insert(6, r"textures\pbr\a\rock01_rmaos.dds".into());
    e.slots.insert(3, String::new());
    let text = to_text(&document(&Settings::new(), &[e]));
    let pos = |k: &str| {
        text.find(&format!("\"{k}\""))
            .unwrap_or_else(|| panic!("{k} in {text}"))
    };
    assert!(pos("match_diffuse") < pos("emissive") && pos("emissive") < pos("specular_level"));
    assert!(pos("specular_level") < pos("slot3") && pos("slot3") < pos("slot6"));
    assert!(
        text.ends_with("}\n") && text.contains("\n  \"default\": {}"),
        "{text}"
    );
    assert!(
        text.contains(r#""slot6": "textures\\pbr\\a\\rock01_rmaos.dds""#),
        "{text}"
    );
}

#[test]
fn a_conflict_through_defaults_keeps_the_stronger_feature() {
    let d = s(&[("multilayer", json!(true))]);
    let v = written(&entry(&[("subsurface", json!(true))], "x"), &d);
    assert_eq!(v["subsurface"], json!(false));
    let doc = document(&d, &[entry(&[], "x")]).to_value();
    assert_eq!(doc["default"]["multilayer"], json!(true));
}

#[test]
fn blocked_objects_are_omitted_and_glint_false_writes_nothing() {
    let d = Settings::new();
    for (label, pairs) in [
        (
            "hair blocks glint",
            vec![("hair", json!(true)), ("glint", json!(true))],
        ),
        (
            "multilayer blocks glint",
            vec![("multilayer", json!(true)), ("glint", json!(true))],
        ),
        (
            "glint blocks fuzz",
            vec![("glint", json!(true)), ("fuzz", json!(true))],
        ),
        (
            "hair blocks fuzz",
            vec![("hair", json!(true)), ("fuzz", json!(true))],
        ),
        (
            "multilayer blocks fuzz",
            vec![("multilayer", json!(true)), ("fuzz", json!(true))],
        ),
    ] {
        let v = written(&entry(&pairs, "x"), &d);
        let blocked = if label.contains("fuzz") {
            "fuzz"
        } else {
            "glint"
        };
        assert!(v.get(blocked).is_none(), "{label}: {v}");
    }
    let v = written(&entry(&[("glint", json!(false))], "x"), &d);
    assert!(v.get("glint").is_none());
    // glint wins over fuzz when both are on; the winner is still written.
    let v = written(
        &entry(&[("glint", json!(true)), ("fuzz", json!(true))], "x"),
        &d,
    );
    assert!(v.get("glint").is_some() && v.get("fuzz").is_none());
}

#[test]
fn dependents_of_a_blocked_feature_are_dropped() {
    let v = written(
        &entry(
            &[
                ("hair", json!(true)),
                ("parallax", json!(true)),
                ("displacement_scale", json!(0.4)),
                ("emissive", json!(true)),
                ("emissive_scale", json!(2.0)),
                ("subsurface", json!(true)),
                ("subsurface_opacity", json!(0.5)),
            ],
            "x",
        ),
        &Settings::new(),
    );
    assert_eq!(v["parallax"], json!(false));
    assert_eq!(v["emissive"], json!(false));
    assert_eq!(v["subsurface"], json!(false));
    for k in ["displacement_scale", "emissive_scale", "subsurface_opacity"] {
        assert!(v.get(k).is_none(), "{k} in {v}");
    }
    assert_eq!(v["hair"], json!(true));
}

#[test]
fn an_entry_switches_a_default_on_only_true_key_off_by_writing_false() {
    let d = s(&[("hair", json!(true))]);
    let v = written(&entry(&[("hair", json!(false))], "x"), &d);
    assert_eq!(v["hair"], json!(false));
    // Unset: the default block carries it. Off with no default on: not written.
    assert!(written(&entry(&[], "x"), &d).get("hair").is_none());
    assert!(
        written(&entry(&[("hair", json!(false))], "x"), &Settings::new())
            .get("hair")
            .is_none()
    );
    // With the entry's own false, the hair-blocked features are live again.
    let v = written(
        &entry(&[("hair", json!(false)), ("parallax", json!(true))], "x"),
        &d,
    );
    assert_eq!(v["parallax"], json!(true));
}

use sdc_gui::pgp::validate::*;

fn keywords_in(v: &Value, out: &mut std::collections::BTreeSet<String>, under_properties: bool) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if !under_properties {
                    out.insert(k.clone());
                }
                // `examples` and `default` hold instance data, not schema.
                if k != "examples" && k != "default" {
                    keywords_in(x, out, k == "properties" || k == "definitions");
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| keywords_in(x, out, false)),
        _ => {}
    }
}

#[test]
fn the_validator_covers_every_keyword_the_schema_uses() {
    let schema: Value = serde_json::from_str(sdc_gui::pgp::schema::SCHEMA_JSON).unwrap();
    let mut used = std::collections::BTreeSet::new();
    keywords_in(&schema, &mut used, false);
    let annotations = [
        "$schema",
        "$id",
        "title",
        "description",
        "examples",
        "default",
        "definitions",
        "properties",
    ];
    for k in used {
        assert!(
            KEYWORDS.contains(&k.as_str()) || annotations.contains(&k.as_str()),
            "the vendored schema uses '{k}', which validate.rs does not implement (DECISIONS P12)"
        );
    }
}

#[test]
fn written_documents_validate_and_bad_ones_do_not() {
    let e = entry(
        &[
            ("emissive", json!(true)),
            ("emissive_color", json!([255, 0, 0])),
            ("glint", json!(true)),
        ],
        "a",
    );
    validate(&document(&s(&[("specular_level", json!(0.04))]), &[e]).to_value()).unwrap();
    // An entry with no match key, a removed key, an unknown key, a wrong type, a short colour.
    for bad in [
        json!({ "default": {}, "entries": [ { "emissive": true } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "pbr": true } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "nope": 1 } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": 3 } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "subsurface_color": [1, 1] } ] }),
        json!({ "entries": [] }),
    ] {
        assert!(validate(&bad).is_err(), "{bad}");
    }
}
