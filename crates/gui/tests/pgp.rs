use sdc_gui::pgp::schema::*;
use sdc_gui::pgp::settings::*;
use sdc_gui::pgp::validate::*;
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
        assert_eq!(spec(k).unwrap().kind, KeyKind::OnlyTrue, "{k}");
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

/// F1 spec §2, DECISIONS V5: every number key and its range, in KEYS order. Keys that fit no class (smooth_angle,
/// auto_uv) get min 0; controller ruling: glint.log_microfacet_density is a log value and has no bounds.
#[test]
fn number_keys_and_their_ranges() {
    use NumRange::{Any, Min0, Unit};
    let got: Vec<(&str, NumRange)> = KEYS
        .iter()
        .filter_map(|k| match k.kind {
            KeyKind::Number(r) => Some((k.key, r)),
            _ => None,
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("smooth_angle", Min0),
            ("auto_uv", Min0),
            ("uv_scale", Min0),
            ("vertex_color_lum_mult", Min0),
            ("vertex_color_sat_mult", Min0),
            ("emissive_scale", Min0),
            ("displacement_scale", Min0),
            ("specular_level", Min0),
            ("roughness_scale", Min0),
            ("subsurface_opacity", Unit),
            ("coat_specular_level", Min0),
            ("coat_roughness", Min0),
            ("coat_strength", Unit),
            ("inner_uv_scale", Min0),
            ("glint.screen_space_scale", Min0),
            ("glint.log_microfacet_density", Any),
            ("glint.microfacet_roughness", Min0),
            ("glint.density_randomization", Min0),
            ("fuzz.weight", Unit),
        ]
    );
    assert_eq!(Min0.clamp(-2.0), 0.0);
    assert_eq!(Min0.clamp(7.5), 7.5);
    assert_eq!(Unit.clamp(1.5), 1.0);
    assert_eq!(Any.clamp(-4.0), -4.0);
    assert_eq!(Min0.note(), "clamped to ≥ 0");
    assert_eq!(Unit.note(), "clamped to 0–1");
}

/// pgpsim keeps its own copy of the ranges (it cannot see the GUI); this pins the two together key for key.
#[test]
fn pgpsim_warns_on_exactly_the_gui_ranges() {
    for k in KEYS {
        let KeyKind::Number(range) = k.kind else {
            continue;
        };
        // Indexing a Null `Value` by a string makes it an object.
        let entry = |x: f64| {
            let mut v = Value::Null;
            match k.key.split_once('.') {
                Some((obj, sub)) => v[obj][sub] = json!(x),
                None => v[k.key] = json!(x),
            }
            v
        };
        assert_eq!(
            pgpsim::range_warnings(&entry(-0.5)).len(),
            usize::from(range != NumRange::Any),
            "{} below 0",
            k.key
        );
        assert!(
            pgpsim::range_warnings(&entry(0.5)).is_empty(),
            "{} inside",
            k.key
        );
        assert_eq!(
            pgpsim::range_warnings(&entry(2.0)).len(),
            usize::from(range == NumRange::Unit),
            "{} above 1",
            k.key
        );
    }
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
    let glint = s(&[("glint_on", json!(true))]);
    assert_eq!(blocked_by(&glint, Fuzz), Some(Glint));
    assert_eq!(blocked_by(&glint, Multilayer), Some(Glint));
    let ss = s(&[("subsurface", json!(true))]);
    assert_eq!(blocked_by(&ss, Multilayer), Some(Subsurface));
    assert_eq!(blocked_by(&ss, Emissive), None);
    // A feature never blocks itself, and off features block nothing.
    assert_eq!(blocked_by(&hair, Hair), None);
    assert_eq!(blocked_by(&s(&[("hair", json!(false))]), Emissive), None);
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
fn the_default_match1_prefixes_and_renames() {
    let p = |pairs: &[(&str, Value)], derived: &str| prefix_of(&s(pairs), derived).unwrap();
    assert_eq!(
        p(&[], r"landscape\rock01"),
        r"textures\pbr\landscape\rock01"
    );
    // PGPatcher cuts the slot-1 suffix from a match1 hit, so a _d source's outputs go under ...\rock01.
    assert_eq!(
        p(&[], r"landscape\rock01_d"),
        r"textures\pbr\landscape\rock01"
    );
    assert_eq!(
        p(&[], r"landscape\rock.v2"),
        r"textures\pbr\landscape\rock.v2"
    );
    assert_eq!(
        p(&[("match1", json!(r"landscape\rock02.dds"))], "x"),
        r"textures\pbr\landscape\rock02"
    );
    assert_eq!(
        p(&[("match1", json!(r"pbr\landscape\rock02"))], "x"),
        r"textures\pbr\landscape\rock02",
        "never doubled"
    );
    assert_eq!(
        p(&[("match1", json!(r"Textures\landscape\rock02"))], "x"),
        r"textures\pbr\landscape\rock02"
    );
    // Rename is a file name in the matched folder (owner, 2026-10-12).
    assert_eq!(
        p(&[("rename", json!("boulder"))], r"landscape\rock01"),
        r"textures\pbr\landscape\boulder"
    );
    // The lowest active match decides; match2 alone gives the same prefix.
    assert_eq!(
        p(
            &[
                ("match1", json!("")),
                ("match2", json!(r"landscape\rock01_n"))
            ],
            "x"
        ),
        r"textures\pbr\landscape\rock01"
    );
    // path_contains alone, or nothing at all: the derived path.
    assert_eq!(
        p(
            &[("match1", json!("")), ("path_contains", json!("rock"))],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\rock01"
    );
    assert_eq!(
        p(&[("match1", json!(""))], r"landscape\rock01"),
        r"textures\pbr\landscape\rock01"
    );
    // Old keys are ignored (F1 spec §3.3).
    assert_eq!(
        p(
            &[
                ("match_diffuse", json!(r"a\b")),
                ("match_normal", json!(r"c\d"))
            ],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\rock01"
    );
}

#[test]
fn slot_modes_strip_the_first_suffix_of_their_own_list() {
    let p = |key: &str, v: &str| {
        prefix_of(&s(&[("match1", json!("")), (key, json!(v))]), "unused").unwrap()
    };
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
    // rename names the file in the folder of the match that decides.
    let r = prefix_of(
        &s(&[
            ("match1", json!("")),
            ("match2", json!(r"a\b_n.dds")),
            ("rename", json!("d")),
        ]),
        "x",
    )
    .unwrap();
    assert_eq!(r, r"textures\pbr\a\d");
}

#[test]
fn bad_paths_block() {
    for bad in [r"..\up", r"C:\abs", r"\lead", "a|b", r"a\..\b"] {
        assert!(
            prefix_of(&s(&[("match1", json!(bad))]), "x").is_err(),
            "{bad:?}"
        );
        assert!(
            prefix_of(&s(&[("rename", json!(bad))]), "x").is_err(),
            "{bad:?}"
        );
        assert!(check_rel_path(bad, "slot3").is_err(), "{bad:?}");
    }
    assert!(check_rel_path("", "slot3").is_err());
    // No match1 key and no derived path: no prefix.
    assert!(prefix_of(&s(&[]), "").is_err());
    assert!(check_rel_path(r"landscape\white.dds", "slot3").is_ok());
}

/// DECISIONS P13 (owner, 2026-10-14): a typed leading separator is dropped before a path is stored.
#[test]
fn a_typed_leading_separator_is_dropped() {
    assert_eq!(strip_lead(r" \\landscape\x "), r"landscape\x");
    assert_eq!(strip_lead("/landscape/x"), "landscape/x");
    assert_eq!(strip_lead(r"landscape\x"), r"landscape\x");
    assert_eq!(
        stored_match(2, r"\landscape\x_n.dds"),
        Some(json!(r"landscape\x_n"))
    );
    for k in ["match1", "match8", "rename"] {
        assert!(is_path_key(k), "{k}");
    }
    for k in ["match9", "match", "path_contains", "nif_filter", "slot3"] {
        assert!(!is_path_key(k), "{k}");
    }
}

/// F1 spec Review Focus 2, DECISIONS V2: .dds exactly once, and no leading textures\ (PGPatcher's prepend check is
/// case-sensitive, so a pasted Textures\ would become textures\Textures\ and never match).
#[test]
fn match_values_are_written_without_textures_and_with_one_dds() {
    for (stored, written) in [
        (r"landscape\rock01", r"landscape\rock01.dds"),
        (r"landscape\rock01.dds", r"landscape\rock01.dds"),
        (r"landscape\rock01.DDS", r"landscape\rock01.DDS"),
        (r"Textures\landscape\rock01.dds", r"landscape\rock01.dds"),
        (r"textures\pbr\x\y", r"pbr\x\y.dds"),
        (r"x\rock.v2", r"x\rock.v2.dds"),
        ("  x\\y  ", r"x\y.dds"),
    ] {
        assert_eq!(match_value(stored), written, "{stored:?}");
    }
    // What a typed value stores: no .dds; empty is off (match1 keeps "" so the derived default stays off).
    assert_eq!(stored_match(1, r"x\y.dds"), Some(json!(r"x\y")));
    assert_eq!(stored_match(3, r" x\y.DDS "), Some(json!(r"x\y")));
    assert_eq!(stored_match(2, r"a\b_n"), Some(json!(r"a\b_n")));
    assert_eq!(stored_match(1, "  "), Some(json!("")));
    assert_eq!(stored_match(4, ""), None);
    // A pasted value with its extension is written once.
    let v = written(&entry(&[("match1", json!(r"landscape\rock01.dds"))], "x"));
    assert_eq!(v["match1"], json!(r"landscape\rock01.dds"));
}

#[test]
fn toggles_and_slot_match_paths() {
    assert_eq!(
        match_toggle(1, true, "ignored"),
        ("match1".to_string(), None)
    );
    assert_eq!(
        match_toggle(1, false, "ignored"),
        ("match1".to_string(), Some(json!("")))
    );
    assert_eq!(
        match_toggle(2, true, r"a\b_n"),
        ("match2".to_string(), Some(json!(r"a\b_n")))
    );
    assert_eq!(
        match_toggle(6, false, r"a\b_rmaos"),
        ("match6".to_string(), None)
    );
    let on = |pairs: &[(&str, Value)]| -> Vec<u8> {
        active_matches(&s(pairs), r"landscape\rock01")
            .iter()
            .map(|(n, _)| *n)
            .collect()
    };
    assert_eq!(on(&[]), vec![1]);
    assert_eq!(on(&[("match1", json!(""))]), Vec::<u8>::new());
    assert_eq!(
        on(&[("match2", json!(r"x\y_n")), ("match6", json!(" "))]),
        vec![1, 2]
    );
    assert!(
        active_matches(&s(&[]), "").is_empty(),
        "no diffuse, no default"
    );
    assert!(matches_nothing(
        &s(&[("match1", json!(""))]),
        r"landscape\rock01"
    ));
    assert!(!matches_nothing(
        &s(&[("match1", json!("")), ("path_contains", json!("rock"))]),
        r"landscape\rock01"
    ));
}

/// Review Focus 3: the slot-1 suffix of the source gives way to the slot's own.
#[test]
fn slot_match_paths_swap_the_diffuse_suffix() {
    assert_eq!(
        slot_match_path(r"landscape\rock01", ""),
        r"landscape\rock01"
    );
    assert_eq!(
        slot_match_path(r"landscape\rock01_d", ""),
        r"landscape\rock01_d"
    );
    assert_eq!(
        slot_match_path(r"landscape\rock01_d", "n"),
        r"landscape\rock01_n"
    );
    assert_eq!(
        slot_match_path(r"landscape\Rock01_D", "n"),
        r"landscape\Rock01_n"
    );
    assert_eq!(
        slot_match_path(r"actors\facemask", "n"),
        r"actors\facemask_n"
    );
    assert_eq!(slot_match_path("d", "n"), "d_n");
    assert_eq!(
        slot_match_path(r"landscape\rock01", "rmaos"),
        r"landscape\rock01_rmaos"
    );
}

#[test]
fn forward_slashes_in_json_paths_block() {
    // PGPatcher never converts '/', and the writer writes these values as typed (DECISIONS P26).
    for key in ["match1", "match2"] {
        let e = prefix_of(&s(&[(key, json!("landscape/rock01"))]), "x").unwrap_err();
        assert!(e.0.contains(key) && e.0.contains(r"use \ not /"), "{e:?}");
    }
    // rename is a file name: a '/' is a folder, refused as such (owner, 2026-10-12).
    let e = prefix_of(&s(&[("rename", json!("landscape/rock01"))]), "x").unwrap_err();
    assert!(
        e.0.contains("rename") && e.0.contains("a file name only"),
        "{e:?}"
    );
    assert!(check_json_path("landscape/white.dds", "slot3").is_err());
    assert!(check_json_path(r"landscape\white.dds", "slot3").is_ok());
    // The derived match is built with backslashes, so a nested folder still works.
    assert_eq!(
        prefix_of(
            &s(&[]),
            &derive_match_diffuse("textures/landscape/rocks", "rock01")
        )
        .unwrap(),
        r"textures\pbr\landscape\rocks\rock01"
    );
}

#[test]
fn pitfalls_are_reported_not_fixed() {
    let w = pitfalls(&s(&[("match8", json!(r"x\leaf_s.dds"))]), "x");
    assert!(w.iter().any(|m| m.contains("match8")), "{w:?}");
    // A rename is a file name now, so there is no "full path" pitfall (owner, 2026-10-12).
    let w = pitfalls(
        &s(&[("match2", json!(r"a\b_n.dds")), ("rename", json!("d"))]),
        r"a\b",
    );
    assert!(w.is_empty(), "{w:?}");
    assert!(pitfalls(&s(&[]), r"landscape\rock01").is_empty());
    let w = pitfalls(&s(&[("match1", json!(""))]), "x");
    assert!(
        w.iter()
            .any(|m| m.starts_with(NO_MATCH) && m.contains("left out")),
        "{w:?}"
    );
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
        section: false,
    }
}

fn written(e: &EntryInput) -> Value {
    document(std::slice::from_ref(e)).to_value()["entries"][0].clone()
}

/// F1 spec §6 "New set": match1 is the derived diffuse path with .dds; match_diffuse and match_normal are never written.
#[test]
fn a_plain_entry_writes_only_match1() {
    let v = written(&entry(&[], r"landscape\rock01"));
    assert_eq!(v, json!({ "match1": r"landscape\rock01.dds" }));
    let old = written(&entry(
        &[
            ("match_diffuse", json!(r"a\b")),
            ("match_normal", json!(r"c\d")),
        ],
        r"landscape\rock01",
    ));
    assert_eq!(
        old,
        json!({ "match1": r"landscape\rock01.dds" }),
        "old keys are ignored"
    );
}

#[test]
fn the_default_block_is_fixed_and_in_schema_order() {
    let doc = document(&[entry(&[], "x")]);
    let Node::Obj(top) = &doc else { panic!() };
    let Node::Obj(block) = &top[1].1 else {
        panic!()
    };
    let keys: Vec<&str> = block.iter().map(|(k, _)| k.as_str()).collect();
    let want: Vec<&str> = KEYS
        .iter()
        .map(|k| k.key)
        .filter(|k| FIXED_FLAGS.contains(k) || FIXED_NUMBERS.iter().any(|(n, _)| n == k))
        .collect();
    assert_eq!(keys, want);
    assert_eq!(keys.len(), 6);
    let v = doc.to_value();
    assert_eq!(
        v["default"],
        json!({ "emissive": false, "parallax": false, "subsurface": false, "roughness_scale": 1.0, "specular_level": 0.04, "displacement_scale": 1.0 })
    );
    assert_eq!(v["$schema"], json!(sdc_gui::pgp::schema::SCHEMA_URL));
    // The block does not depend on the entries.
    assert_eq!(document(&[]).to_value()["default"], v["default"]);
}

#[test]
fn fixed_numbers_are_omitted_when_unset_or_equal_and_written_otherwise() {
    let none = written(&entry(&[], "x"));
    for k in ["roughness_scale", "specular_level", "displacement_scale"] {
        assert!(none.get(k).is_none(), "{k}");
    }
    let v = written(&entry(
        &[
            ("specular_level", json!(0.04)),
            ("roughness_scale", json!(1.0)),
        ],
        "x",
    ));
    assert!(v.get("specular_level").is_none() && v.get("roughness_scale").is_none());
    let v = written(&entry(
        &[
            ("specular_level", json!(0.06)),
            ("roughness_scale", json!(0.5)),
        ],
        "x",
    ));
    assert_eq!(v["specular_level"], json!(0.06));
    assert_eq!(v["roughness_scale"], json!(0.5));
    // displacement_scale follows parallax: equal to the fixed value it is omitted, otherwise written.
    let p = |d: f64| {
        written(&entry(
            &[("parallax", json!(true)), ("displacement_scale", json!(d))],
            "x",
        ))
    };
    assert!(p(1.0).get("displacement_scale").is_none());
    assert_eq!(p(0.4)["displacement_scale"], json!(0.4));
}

/// F1 spec §2: a hand-edited out-of-range value cannot reach the JSON; an in-range value is written as it was typed.
#[test]
fn the_writer_clamps_a_hand_edited_number_into_its_range() {
    let v = written(&entry(
        &[
            ("roughness_scale", json!(-2.0)),
            ("specular_level", json!(-0.1)),
            ("multilayer", json!(true)),
            ("coat_strength", json!(3)),
            ("coat_roughness", json!(-1)),
            ("coat_specular_level", json!(2)),
        ],
        "x",
    ));
    assert_eq!(v["roughness_scale"], json!(0.0));
    assert_eq!(v["specular_level"], json!(0.0));
    assert_eq!(v["coat_strength"], json!(1.0));
    assert_eq!(v["coat_roughness"], json!(0.0));
    assert_eq!(v["coat_specular_level"], json!(2), "in range: as typed");
    assert!(pgpsim::range_warnings(&v).is_empty(), "{v}");
    let g = written(&entry(
        &[
            ("glint_on", json!(true)),
            ("glint.microfacet_roughness", json!(-0.5)),
            ("glint.log_microfacet_density", json!(-3.0)),
        ],
        "x",
    ));
    assert_eq!(g["glint"]["microfacet_roughness"], json!(0.0));
    assert_eq!(
        g["glint"]["log_microfacet_density"],
        json!(-3.0),
        "a log value may be negative"
    );
    let f = written(&entry(
        &[("fuzz_on", json!(true)), ("fuzz.weight", json!(1.5))],
        "x",
    ));
    assert_eq!(f["fuzz"]["weight"], json!(1.0));
    assert!(pgpsim::range_warnings(&g).is_empty() && pgpsim::range_warnings(&f).is_empty());
}

#[test]
fn emissive_parallax_and_subsurface_are_written_only_as_true() {
    for k in ["emissive", "parallax", "subsurface"] {
        assert!(written(&entry(&[], "x")).get(k).is_none(), "{k} unset");
        assert!(
            written(&entry(&[(k, json!(false))], "x")).get(k).is_none(),
            "{k} off"
        );
        assert_eq!(written(&entry(&[(k, json!(true))], "x"))[k], json!(true));
        assert!(
            written(&entry(&[(k, json!(true)), ("hair", json!(true))], "x"))
                .get(k)
                .is_none(),
            "{k} blocked by hair"
        );
    }
    // Multilayer blocks subsurface only.
    let ml = |k: &str| {
        written(&entry(
            &[(k, json!(true)), ("multilayer", json!(true))],
            "x",
        ))
    };
    assert!(ml("subsurface").get("subsurface").is_none());
    assert_eq!(ml("parallax")["parallax"], json!(true));
}

#[test]
fn colours_convert_and_emissive_gets_alpha_one() {
    let v = written(&entry(
        &[
            ("emissive", json!(true)),
            ("emissive_color", json!([255, 128, 0])),
            ("emissive_scale", json!(2.0)),
        ],
        "x",
    ));
    assert_eq!(v["emissive_color"], json!([1.0, 0.502, 0.0, 1.0]));
    assert_eq!(v["emissive_scale"], json!(2.0));
}

#[test]
fn dependent_keys_need_their_feature() {
    let v = written(&entry(
        &[
            ("emissive_scale", json!(2.0)),
            ("displacement_scale", json!(0.4)),
        ],
        "x",
    ));
    assert!(v.get("emissive_scale").is_none() && v.get("displacement_scale").is_none());
}

#[test]
fn blocked_features_are_not_written() {
    let v = written(&entry(
        &[
            ("hair", json!(true)),
            ("parallax", json!(true)),
            ("multilayer", json!(true)),
        ],
        "x",
    ));
    assert_eq!(v["hair"], json!(true));
    assert!(v.get("parallax").is_none());
    assert!(v.get("multilayer").is_none());
}

#[test]
fn glint_is_filled_from_its_defaults_and_fuzz_writes_what_is_set() {
    let v = written(&entry(
        &[
            ("glint_on", json!(true)),
            ("glint.microfacet_roughness", json!(0.02)),
        ],
        "x",
    ));
    assert_eq!(
        v["glint"],
        json!({ "screen_space_scale": 1.5, "log_microfacet_density": 40.0, "microfacet_roughness": 0.02, "density_randomization": 2.0 })
    );
    let v = written(&entry(
        &[
            ("fuzz_on", json!(true)),
            ("fuzz.texture", json!(true)),
            ("fuzz.color", json!([0, 0, 0])),
        ],
        "x",
    ));
    assert_eq!(
        v["fuzz"],
        json!({ "texture": true, "color": [0.0, 0.0, 0.0] })
    );
}

#[test]
fn match_slots_are_written_with_dds_and_tri_state_writes_only_when_set() {
    let v = written(&entry(
        &[
            ("match_diffuse", json!("old")),
            ("match2", json!(r"a\b_n")),
            ("lock_rmaos", json!(false)),
        ],
        "x",
    ));
    assert!(v.get("match_diffuse").is_none() && v.get("match_normal").is_none());
    assert_eq!(v["match1"], json!("x.dds"));
    assert_eq!(v["match2"], json!(r"a\b_n.dds"));
    assert_eq!(v["lock_rmaos"], json!(false));
    assert!(v.get("lock_diffuse").is_none());
}

#[test]
fn slots_and_key_order() {
    let mut e = entry(
        &[("emissive", json!(true)), ("specular_level", json!(0.06))],
        "x",
    );
    e.slots.insert(6, r"textures\pbr\a\rock01_rmaos.dds".into());
    e.slots.insert(3, String::new());
    let text = to_text(&document(&[e]));
    let entries = text.find("\"entries\"").unwrap();
    let pos = |k: &str| {
        text[entries..]
            .find(&format!("\"{k}\""))
            .unwrap_or_else(|| panic!("{k} in {text}"))
    };
    assert!(pos("match1") < pos("emissive") && pos("emissive") < pos("specular_level"));
    assert!(pos("specular_level") < pos("slot3") && pos("slot3") < pos("slot6"));
    assert!(
        text.ends_with("}\n") && text.contains("\n  \"default\": {\n"),
        "{text}"
    );
    assert!(
        text.contains(r#""slot6": "textures\\pbr\\a\\rock01_rmaos.dds""#),
        "{text}"
    );
}

#[test]
fn a_conflict_inside_one_set_keeps_the_stronger_feature() {
    let v = written(&entry(
        &[("multilayer", json!(true)), ("subsurface", json!(true))],
        "x",
    ));
    assert_eq!(v["multilayer"], json!(true));
    assert!(v.get("subsurface").is_none());
}

#[test]
fn blocked_objects_are_omitted_and_glint_false_writes_nothing() {
    for (label, pairs) in [
        (
            "hair blocks glint",
            vec![("hair", json!(true)), ("glint_on", json!(true))],
        ),
        (
            "multilayer blocks glint",
            vec![("multilayer", json!(true)), ("glint_on", json!(true))],
        ),
        (
            "glint blocks fuzz",
            vec![("glint_on", json!(true)), ("fuzz_on", json!(true))],
        ),
        (
            "hair blocks fuzz",
            vec![("hair", json!(true)), ("fuzz_on", json!(true))],
        ),
        (
            "multilayer blocks fuzz",
            vec![("multilayer", json!(true)), ("fuzz_on", json!(true))],
        ),
    ] {
        let v = written(&entry(&pairs, "x"));
        let blocked = if label.contains("fuzz") {
            "fuzz"
        } else {
            "glint"
        };
        assert!(v.get(blocked).is_none(), "{label}: {v}");
    }
    let v = written(&entry(&[("glint_on", json!(false))], "x"));
    assert!(v.get("glint").is_none());
    // glint wins over fuzz when both are on; the winner is still written.
    let v = written(&entry(
        &[("glint_on", json!(true)), ("fuzz_on", json!(true))],
        "x",
    ));
    assert!(v.get("glint").is_some() && v.get("fuzz").is_none());
}

#[test]
fn dependents_of_a_blocked_feature_are_dropped() {
    let v = written(&entry(
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
    ));
    for k in [
        "parallax",
        "emissive",
        "subsurface",
        "displacement_scale",
        "emissive_scale",
        "subsurface_opacity",
    ] {
        assert!(v.get(k).is_none(), "{k} in {v}");
    }
    assert_eq!(v["hair"], json!(true));
}

#[test]
fn hair_with_parallax_writes_no_parallax_and_no_displacement_scale() {
    let v = written(&entry(
        &[
            ("hair", json!(true)),
            ("parallax", json!(true)),
            ("displacement_scale", json!(0.4)),
        ],
        "x",
    ));
    assert!(v.get("parallax").is_none() && v.get("displacement_scale").is_none());
    assert_eq!(v["hair"], json!(true));
}

fn keywords_in(v: &Value, out: &mut std::collections::BTreeSet<String>, under_properties: bool) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if !under_properties {
                    out.insert(k.clone());
                }
                // `examples` and `default` hold instance data, not schema.
                if under_properties || (k != "examples" && k != "default") {
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
            ("glint_on", json!(true)),
        ],
        "a",
    );
    validate(&document(&[e]).to_value()).unwrap();
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

#[test]
fn failures_name_the_path_and_the_key() {
    let doc = json!({ "default": {}, "entries": [ { "match_diffuse": "a" }, { "match_diffuse": "b", "nope": 1 } ] });
    let errs = validate(&doc).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("entries[1]") && e.contains("nope")),
        "{errs:?}"
    );
}

#[test]
fn slot_suffixes_compare_ascii_only() {
    // The Kelvin sign lower-cases to an ASCII 'k'; it is not the `_sk` suffix and must not be cut (nor panic).
    let p = prefix_of(
        &s(&[
            ("match1", json!("")),
            ("match3", json!("x\\a_s\u{212A}.dds")),
        ]),
        "unused",
    )
    .unwrap();
    assert_eq!(p, "textures\\pbr\\x\\a_s\u{212A}");
    let p = prefix_of(
        &s(&[
            ("match1", json!("")),
            ("match3", json!("x\\\u{212A}\u{212A}_SK.dds")),
        ]),
        "unused",
    )
    .unwrap();
    assert_eq!(p, "textures\\pbr\\x\\\u{212A}\u{212A}");
}

/// F1 spec Review Focus 1, DECISIONS V1: matchN holds the path the unpatched mesh carries. PGPatcher hits it, and
/// its matched path is the prefix sdc writes the outputs under. `derive_match_diffuse` is called with the stem that
/// `set_views` gives it: `mirrored_stem` cuts only colour vocabulary terms, and `d` is not one, so `rock01_d` stays.
#[test]
fn slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix() {
    let default = document(&[]).to_value()["default"].clone();
    let mesh = |d: &str, n: &str| {
        let mut m = pgpsim::MaterialState::builtin_default();
        m.set_slot_raw(0, d);
        m.set_slot_raw(1, n);
        m
    };
    let vanilla = mesh(
        r"textures\landscape\rock01.dds",
        r"textures\landscape\rock01_n.dds",
    );
    let d_named = mesh(
        r"Textures\Landscape\Rock01_D.dds",
        r"textures\landscape\rock01_n.dds",
    );
    let nif = r"meshes\landscape\rock01.nif";
    let cases = [
        (
            "match1, the default",
            "textures/landscape",
            "rock01",
            vec![],
            &vanilla,
        ),
        (
            "a pbr source folder still names the vanilla path",
            "textures/pbr/landscape",
            "rock01",
            vec![],
            &vanilla,
        ),
        (
            "match2 alone",
            "textures/landscape",
            "rock01",
            vec![
                ("match1", json!("")),
                ("match2", json!(slot_match_path(r"landscape\rock01", "n"))),
            ],
            &vanilla,
        ),
        (
            "match1 and match2",
            "textures/landscape",
            "rock01",
            vec![("match2", json!(r"landscape\rock01_n"))],
            &vanilla,
        ),
        (
            "a _d source keeps _d in match1",
            "textures/landscape",
            "rock01_d",
            vec![],
            &d_named,
        ),
    ];
    for (label, dir, stem, pairs, shape) in cases {
        let derived = derive_match_diffuse(dir, stem);
        let e = entry(&pairs, &derived);
        let prepared = pgpsim::prepare(&written(&e), &default);
        assert!(
            pgpsim::shape_matches(&prepared, nif, &shape.slots),
            "{label}"
        );
        assert_eq!(
            pgpsim::matched_path(&prepared, &shape.slots),
            Some(
                prefix_of(&e.settings, &derived)
                    .unwrap()
                    .to_ascii_lowercase()
            ),
            "{label}"
        );
    }
    // matchN is exact (no strip_pbr): a mesh already pointing at textures\pbr\ is not hit. PGPatcher patches the
    // unpatched copy, which carries the vanilla path.
    let patched = mesh(
        r"textures\pbr\landscape\rock01.dds",
        r"textures\pbr\landscape\rock01_n.dds",
    );
    let prepared = pgpsim::prepare(&written(&entry(&[], r"landscape\rock01")), &default);
    assert!(!pgpsim::shape_matches(&prepared, nif, &patched.slots));
}

/// Task 3 review fix: PGPatcher takes the prefix from whichever match hits, not the lowest active one. When the active
/// matches derive different prefixes the set warns (advising to edit the match paths). With a rename it still warns:
/// PGPatcher cuts the hit's path by `match_diffuse`'s length, so only the first matched texture is sure to land (AA8).
#[test]
fn divergent_matches_warn_and_a_rename_pins_the_prefix() {
    let default = document(&[]).to_value()["default"].clone();
    let mut mesh = pgpsim::MaterialState::builtin_default();
    mesh.set_slot_raw(0, r"textures\c\other.dds");
    mesh.set_slot_raw(1, r"textures\b\two_n.dds");
    let nif = r"meshes\b\two.nif";
    let pairs = [("match2", json!(r"b\two_n"))];
    let e = entry(&pairs, r"a\one");
    let prepared = pgpsim::prepare(&written(&e), &default);
    assert!(pgpsim::shape_matches(&prepared, nif, &mesh.slots));
    // Only match2 hits, so PGPatcher's prefix is not the one sdc writes under.
    assert_ne!(
        pgpsim::matched_path(&prepared, &mesh.slots),
        Some(
            prefix_of(&e.settings, r"a\one")
                .unwrap()
                .to_ascii_lowercase()
        )
    );
    let w = pitfalls(&e.settings, r"a\one");
    assert!(
        w.iter()
            .any(|m| m.contains("match slots point at different textures")
                && m.contains("edit the match paths so they point at the same texture")
                && !m.contains("Rename")),
        "{w:?}"
    );
    // With a rename the split is still reported, and a hit through another texture's path lands elsewhere.
    let pairs = [("match2", json!(r"b\other_n")), ("rename", json!("one"))];
    let e = entry(&pairs, r"a\one");
    let w = pitfalls(&e.settings, r"a\one");
    assert!(w.iter().any(|m| m.contains("with a rename")), "{w:?}");
    let mut other = pgpsim::MaterialState::builtin_default();
    other.set_slot_raw(0, r"textures\c\x.dds");
    other.set_slot_raw(1, r"textures\b\other_n.dds");
    let prepared = pgpsim::prepare(&written(&e), &default);
    assert_ne!(
        pgpsim::matched_path(&prepared, &other.slots),
        Some(r"textures\pbr\a\one".into())
    );
    // A hit through the first matched texture lands on sdc's prefix exactly.
    let mut first = pgpsim::MaterialState::builtin_default();
    first.set_slot_raw(0, r"textures\a\one.dds");
    assert_eq!(
        pgpsim::matched_path(&prepared, &first.slots),
        Some(r"textures\pbr\a\one".into())
    );
    // The default: match1 and match2 from the same set agree, so no warning.
    let same = s(&[("match2", json!(slot_match_path(r"landscape\rock01_d", "n")))]);
    assert!(pitfalls(&same, r"landscape\rock01_d").is_empty());
}

/// Owner, 2026-10-12 (final fix wave): Rename is a new file name. sdc puts it in the folder of the matched path and
/// writes the full prefix, the form PGPatcher's whole-match substitution needs, for every match mode.
#[test]
fn a_rename_is_a_file_name_in_the_matched_folder() {
    let p = |pairs: &[(&str, Value)], derived: &str| prefix_of(&s(pairs), derived).unwrap();
    assert_eq!(
        p(&[("rename", json!("boulder"))], r"landscape\rock01"),
        r"textures\pbr\landscape\boulder"
    );
    assert_eq!(
        p(&[("rename", json!("boulder.DDS"))], r"landscape\rock01"),
        r"textures\pbr\landscape\boulder",
        "one .dds is cut, like a match path"
    );
    assert_eq!(
        p(
            &[
                ("match1", json!("")),
                ("match2", json!(r"b\two_n")),
                ("rename", json!("one"))
            ],
            r"a\one"
        ),
        r"textures\pbr\b\one",
        "the folder of the lowest active match"
    );
    assert_eq!(
        p(
            &[
                ("match1", json!("")),
                ("path_contains", json!("rock")),
                ("rename", json!("snow01"))
            ],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\snow01",
        "path_contains: the folder of the derived path"
    );
    for bad in [r"landscape\boulder", "landscape/boulder", r"textures\pbr\x"] {
        let e = prefix_of(&s(&[("rename", json!(bad))]), r"landscape\rock01").unwrap_err();
        assert!(
            e.0.contains("rename") && e.0.contains("a file name only"),
            "{bad:?}: {e:?}"
        );
    }
    assert!(prefix_of(&s(&[("rename", json!(".dds"))]), r"landscape\rock01").is_err());
    // The JSON carries the matched texture and the renamed path, both under textures\pbr\ (DECISIONS AA8).
    let v = written(&entry(&[("rename", json!("boulder"))], r"landscape\rock01"));
    assert_eq!(v["match_diffuse"], json!(r"landscape\rock01"));
    assert_eq!(v["rename"], json!(r"landscape\boulder"));
    // The match slots stay the entry's matches (owner, 2026-10-14).
    assert_eq!(v["match1"], json!(r"landscape\rock01.dds"));
    assert!(written(&entry(&[], r"landscape\rock01"))
        .get("match_diffuse")
        .is_none());
}

/// The renamed entry lands where sdc writes the files, through pgpsim, with no leading `\` (DECISIONS AA8). Without
/// `match_diffuse`, PGPatcher swaps `\` + rename in for the whole path: real PGPatcher 1.2.0 then patched nothing (owner's
/// robesf2 test, 2026-10-14).
#[test]
fn a_renamed_entry_lands_on_the_sdc_prefix_for_slot_matches_and_path_contains() {
    let default = document(&[]).to_value()["default"].clone();
    let mut mesh = pgpsim::MaterialState::builtin_default();
    mesh.set_slot_raw(0, r"textures\landscape\rock01.dds");
    mesh.set_slot_raw(1, r"textures\landscape\rock01_n.dds");
    let nif = r"meshes\landscape\rock01.nif";
    let derived = r"landscape\rock01";
    let cases: [(&str, Vec<(&str, Value)>); 3] = [
        ("slot match", vec![("rename", json!("boulder"))]),
        (
            "path_contains alone",
            vec![
                ("match1", json!("")),
                ("path_contains", json!("rock01")),
                ("rename", json!("boulder")),
            ],
        ),
        (
            "path_contains with a slot match",
            vec![
                ("path_contains", json!("rock01")),
                ("match2", json!(r"landscape\rock01_n")),
                ("rename", json!("boulder")),
            ],
        ),
    ];
    for (label, pairs) in cases {
        let e = entry(&pairs, derived);
        let prepared = pgpsim::prepare(&written(&e), &default);
        assert!(
            pgpsim::shape_matches(&prepared, nif, &mesh.slots),
            "{label}"
        );
        let want = prefix_of(&e.settings, derived).unwrap();
        assert_eq!(want, r"textures\pbr\landscape\boulder", "{label}");
        assert_eq!(
            pgpsim::matched_path(&prepared, &mesh.slots),
            Some(want.to_ascii_lowercase()),
            "{label}"
        );
    }
    // The old form, without `match_diffuse`: the whole path is replaced and starts with `\`.
    let mut old = written(&entry(&[("rename", json!("boulder"))], derived));
    old.as_object_mut().unwrap().remove("match_diffuse");
    old["rename"] = json!(r"textures\pbr\landscape\boulder");
    assert_eq!(
        pgpsim::matched_path(&pgpsim::prepare(&old, &default), &mesh.slots),
        Some(r"\textures\pbr\landscape\boulder".into())
    );
}

/// Owner bullet B (slot 8 `_s`), checked against pgpsim: PGPatcher's slot-8 list is `_b`/`_bl`/`_flow`, so a mesh
/// hit through `match8 = …_s` gets a prefix ending in `_s`, not sdc's. The two warnings report a real split.
#[test]
fn a_match8_on_an_s_map_keeps_the_s_in_pgpatchers_prefix() {
    let default = document(&[]).to_value()["default"].clone();
    let mut mesh = pgpsim::MaterialState::builtin_default();
    mesh.set_slot_raw(0, r"textures\other\x.dds");
    mesh.set_slot_raw(7, r"textures\landscape\rock01_s.dds");
    let e = entry(
        &[("match8", json!(r"landscape\rock01_s"))],
        r"landscape\rock01",
    );
    let prepared = pgpsim::prepare(&written(&e), &default);
    assert_eq!(
        pgpsim::matched_path(&prepared, &mesh.slots),
        Some(r"textures\pbr\landscape\rock01_s".into())
    );
    assert_eq!(
        prefix_of(&e.settings, r"landscape\rock01").unwrap(),
        r"textures\pbr\landscape\rock01"
    );
}

/// Owner, 2026-10-12: a + row added for a slot the set does not fill stores `matchN = ""`. It is not a match: it is
/// not written, and it does not change the prefix or the warnings.
#[test]
fn an_empty_match_row_is_not_written() {
    let e = entry(&[("match3", json!(""))], r"landscape\rock01");
    assert_eq!(written(&e), json!({ "match1": r"landscape\rock01.dds" }));
    assert!(pitfalls(&e.settings, r"landscape\rock01").is_empty());
}

/// Audit 2026-10-13b G-I1 (DECISIONS AF8): PGPatcher matches `match_diffuse` by its last path components, so a
/// renamed set whose match is short or has no folder also hits same-named textures in other folders (and PGPatcher
/// skips those meshes: the renamed file does not exist there). A pitfall says so when a known texture shows it.
#[test]
fn a_short_renamed_match_warns_when_the_name_is_known_in_another_folder() {
    use sdc_gui::pgp::prefix::rename_hits_elsewhere;
    let renamed = s(&[("rename", json!("iron2"))]);
    let known: Vec<String> = ["textures/armor/iron", "textures/iron", "textures/pbr/iron"]
        .map(String::from)
        .to_vec();
    let w = rename_hits_elsewhere(&renamed, "iron", &known).expect("warns");
    assert!(
        w.contains("textures/armor/iron") && w.contains("skip"),
        "{w}"
    );
    // Its own paths (vanilla and PBR) only: no warning.
    assert_eq!(rename_hits_elsewhere(&renamed, "iron", &known[1..]), None);
    // Not renamed: no match_diffuse, no warning.
    assert_eq!(rename_hits_elsewhere(&s(&[]), "iron", &known), None);
    // A folder in the match narrows it; a longer path ending with it is still hit.
    assert_eq!(rename_hits_elsewhere(&renamed, r"armor\iron", &known), None);
    let deeper = vec!["textures/dlc01/armor/iron".to_string()];
    assert!(rename_hits_elsewhere(&renamed, r"armor\iron", &deeper).is_some());
}
