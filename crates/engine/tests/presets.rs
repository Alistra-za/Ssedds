use engine::codes::{Code, Severity};
use engine::config::*;
use engine::overrides::to_patch;
use engine::presets::*;
use serde_json::json;

#[test]
fn merge_is_deep_and_leaf_paths_are_dotted() {
    let mut a = json!({"alpha": {"policy": "remove", "threshold": 127}, "format": "bc7"});
    merge_json(&mut a, &json!({"alpha": {"policy": "data"}}));
    assert_eq!(
        a,
        json!({"alpha": {"policy": "data", "threshold": 127}, "format": "bc7"})
    );
    let mut paths = vec![];
    leaf_paths(
        &json!({"alpha": {"policy": "data"}, "format": "bc1"}),
        "",
        &mut paths,
    );
    paths.sort();
    assert_eq!(paths, vec!["alpha.policy", "format"]);
}

#[test]
fn null_patches_never_erase() {
    let mut a = json!({"format": "bc7", "alpha": {"policy": "remove"}});
    merge_json(&mut a, &json!({"format": null, "alpha": null}));
    assert_eq!(a, json!({"format": "bc7", "alpha": {"policy": "remove"}}));
}

#[test]
fn builtin_defaults_match_spec_table_4_2() {
    let r = PresetRegistry::builtin().unwrap();
    let p = r
        .profile("cs-quality", Target::Cs, Subtype::PbrAlbedo)
        .unwrap();
    assert_eq!(
        (p.format, p.color.tag, p.color.content),
        (Format::Bc7, ColorTag::Srgb, ColorContent::Srgb)
    );
    let p = r
        .profile("vanilla-quality", Target::Vanilla, Subtype::Diffuse)
        .unwrap();
    assert_eq!((p.format, p.color.tag), (Format::Bc7, ColorTag::Unorm));
    let p = r.profile("cs-quality", Target::Cs, Subtype::Flow).unwrap();
    assert_eq!((p.format, p.encoder.backend), (Format::Bc5, Backend::Rgbcx));
    let p = r
        .profile("cs-quality", Target::Cs, Subtype::Envmask)
        .unwrap();
    assert_eq!(
        (p.format, p.alpha.policy, p.alpha.exact_opaque),
        (Format::Bc4, AlphaPolicy::Remove, true)
    );
    let p = r
        .profile("cs-quality", Target::Cs, Subtype::ComplexMaterial)
        .unwrap();
    assert_eq!(
        (p.alpha.policy, p.alpha.exact_opaque),
        (AlphaPolicy::Data, true)
    );
    let p = r
        .profile("cs-quality", Target::Cs, Subtype::PbrEmissive)
        .unwrap();
    assert_eq!(
        (p.format, p.color.tag, p.encoder.backend),
        (Format::Bc6hUf16, ColorTag::NotApplicable, Backend::IspcBc6h)
    );
    let p = r
        .profile("cs-quality", Target::Cs, Subtype::Normal)
        .unwrap();
    assert_eq!(
        (
            p.alpha.policy,
            p.alpha.threshold,
            p.encoder.effort,
            p.encoder.perceptual
        ),
        (AlphaPolicy::Data, 127, Effort::Veryslow, false)
    );
}

#[test]
fn every_builtin_subtype_profile_deserialises() {
    let r = PresetRegistry::builtin().unwrap();
    for (id, target) in [
        ("cs-quality", Target::Cs),
        ("cpm-quality", Target::Cpm),
        ("vanilla-quality", Target::Vanilla),
    ] {
        for st in Subtype::ALL {
            let p = r
                .profile(id, target, st)
                .unwrap_or_else(|e| panic!("{id}/{st:?}: {e}"));
            assert_eq!(
                (p.target, p.subtype, p.category),
                (target, st, st.category())
            );
        }
    }
}

#[test]
fn preset_targets_and_default_preset_agree() {
    let r = PresetRegistry::builtin().unwrap();
    for t in [Target::Cs, Target::Cpm, Target::Vanilla] {
        assert_eq!(r.get(default_preset_for(t)).unwrap().target, Some(t));
    }
    assert_eq!(default_preset_for(Target::Cs), DEFAULT_PRESET);
    assert!(r.get(BASE_ID).unwrap().hidden);
    let ids: Vec<_> = r.selectable().iter().map(|p| p.id.clone()).collect();
    assert_eq!(ids, ["cpm-quality", "cs-quality", "vanilla-quality"]);
    assert_eq!(r.get(BASE_ID).unwrap().rules.len(), 2);
}

#[test]
fn chain_inheritance_and_errors() {
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(
        r.chain("cs-quality")
            .unwrap()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["sdc-base", "cs-quality"]
    );
    assert!(r.selectable().iter().all(|p| p.id != "sdc-base"));
    let id = r
        .add_toml("schema_version = 1\nid = \"mine\"\nversion = \"0.1.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n")
        .unwrap();
    assert_eq!(
        r.profile(&id, Target::Cs, Subtype::Diffuse).unwrap().format,
        Format::Bc1
    );
    assert!(matches!(
        r.add_toml("schema_version = 2\nid = \"x\"\nversion = \"1.0.0\"\n"),
        Err(PresetError::NewerSchema(2))
    ));
    assert!(matches!(r.chain("nope"), Err(PresetError::Unknown(_))));
    let bad = r.add_toml("schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\nbase = \"sdc-base\"\n[defaults]\nformat = \"bc9\"\n").unwrap();
    assert!(matches!(
        r.profile(&bad, Target::Cs, Subtype::Diffuse),
        Err(PresetError::Invalid(_))
    ));
    assert_eq!(r.sha256("cs-quality").unwrap().len(), 64);
    assert_eq!(r.sha256("nope"), None);
}

#[test]
fn inheritance_cycle_is_detected() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml("schema_version = 1\nid = \"a\"\nversion = \"1\"\nbase = \"b\"\n")
        .unwrap();
    r.add_toml("schema_version = 1\nid = \"b\"\nversion = \"1\"\nbase = \"a\"\n")
        .unwrap();
    assert!(matches!(r.chain("a"), Err(PresetError::Cycle(_))));
}

#[test]
fn layers_put_all_defaults_before_all_subtype_tables() {
    let mut r = PresetRegistry::builtin().unwrap();
    let id = r
        .add_toml("schema_version = 1\nid = \"child\"\nversion = \"1\"\nbase = \"cs-quality\"\n[defaults]\nformat = \"bc1\"\n")
        .unwrap();
    let layers = r.layers(&id, Subtype::Envmask).unwrap();
    let provs: Vec<_> = layers.iter().map(|(p, _)| p.to_string()).collect();
    // base defaults, child defaults, then base's envmask table (cs-quality has none).
    assert_eq!(provs, ["builtin", "preset:child", "builtin"]);
    // A child generic default does NOT override the base envmask sub-type format.
    assert_eq!(
        r.profile(&id, Target::Cs, Subtype::Envmask).unwrap().format,
        Format::Bc4
    );
    // ... but does apply to sub-types with no format of their own.
    assert_eq!(
        r.profile(&id, Target::Cs, Subtype::Diffuse).unwrap().format,
        Format::Bc1
    );
}

#[test]
fn provenance_serialises_as_spec_strings() {
    let cases = [
        (Provenance::Builtin, "builtin"),
        (Provenance::Preset("cs-quality".into()), "preset:cs-quality"),
        (Provenance::UserConfig, "user_config"),
        (Provenance::ProjectConfig, "project_config"),
        (Provenance::Rule("sdc.toml#3".into()), "rule:sdc.toml#3"),
        (Provenance::Analysis, "analysis"),
        (
            Provenance::AnalysisFact("float-source".into()),
            "analysis:float-source",
        ),
        (Provenance::Suggestion, "suggestion"),
        (Provenance::User, "user"),
        (Provenance::Fallback, "fallback"),
    ];
    for (p, s) in cases {
        assert_eq!(serde_json::to_value(&p).unwrap(), json!(s));
        assert_eq!(serde_json::from_value::<Provenance>(json!(s)).unwrap(), p);
    }
    assert!(serde_json::from_value::<Provenance>(json!("bogus")).is_err());
    assert!(Provenance::Builtin < Provenance::Preset("a".into()));
    assert!(Provenance::Suggestion < Provenance::User);
    // Spec §5.2's `set` (set-level decisions, milestone M4) is not a value yet (final review m5).
    assert!(serde_json::from_value::<Provenance>(json!("set")).is_err());
}

#[test]
fn config_file_loads_and_rejects_unknown_keys() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sdc.toml");
    std::fs::write(
        &p,
        "preset = \"cs-quality\"\ntarget = \"cs\"\n[defaults]\nformat = \"bc7\"\n[subtypes.diffuse]\nalpha = { policy = \"cutout\" }\n[[rules]]\nmatch = { glob = \"**/armor/**\", alpha = \"varying\" }\nsubtype = \"diffuse\"\noverrides = { alpha = { policy = \"blend\" } }\n",
    )
    .unwrap();
    let c = ConfigFile::load(&p).unwrap();
    assert_eq!(c.preset.as_deref(), Some("cs-quality"));
    assert_eq!(c.rules.len(), 1);
    assert_eq!(c.rules[0].matcher.alpha, Some(AlphaContent::Varying));
    assert_eq!(c.rules[0].subtype, Some(Subtype::Diffuse));
    assert!(c.subtypes.contains_key(&Subtype::Diffuse));
    std::fs::write(&p, "bogus = 1\n").unwrap();
    assert!(matches!(ConfigFile::load(&p), Err(PresetError::Parse(_))));
    assert!(matches!(
        ConfigFile::load(&dir.path().join("missing.toml")),
        Err(PresetError::Parse(_))
    ));
}

#[test]
fn add_file_registers_a_preset() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("mine.toml");
    std::fs::write(
        &p,
        "schema_version = 1\nid = \"filed\"\nversion = \"1\"\nbase = \"sdc-base\"\n",
    )
    .unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(r.add_file(&p).unwrap(), "filed");
    assert!(r.get("filed").is_some());
}

#[test]
fn finish_profile_reports_invalid_documents() {
    let e = finish_profile(json!({"format": "bc9"}), Target::Cs, Subtype::Diffuse).unwrap_err();
    assert!(matches!(e, PresetError::Invalid(_)));
}

/// Codex #2: `sdc-base` is reserved; a user preset cannot replace it.
#[test]
fn the_base_preset_id_is_reserved() {
    let mut r = PresetRegistry::builtin().unwrap();
    let before = r.get(BASE_ID).unwrap().clone();
    let e = r
        .add_toml(
            "schema_version = 1
id = \"sdc-base\"
version = \"1\"
",
        )
        .unwrap_err();
    assert!(e.to_string().starts_with("CFG-INVALID"), "{e}");
    assert_eq!(r.get(BASE_ID), Some(&before));
    assert!(r.is_builtin(BASE_ID));
    let dir = std::env::temp_dir().join(format!("sdc-reserved-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("base.toml");
    std::fs::write(
        &f,
        "schema_version = 1
id = \"sdc-base\"
version = \"1\"
",
    )
    .unwrap();
    assert!(r
        .add_file(&f)
        .unwrap_err()
        .to_string()
        .starts_with("CFG-INVALID"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(r.get(BASE_ID), Some(&before));
}

/// M2.5 (spec §6.4): packing settings are profile fields with the neutral defaults of the base preset.
#[test]
fn packing_fields_default_from_the_base_preset() {
    let reg = PresetRegistry::builtin().unwrap();
    let p = reg
        .profile("cs-quality", Target::Cs, Subtype::Rmaos)
        .unwrap();
    assert_eq!(
        p.packing,
        PackingSpec {
            enabled: true,
            roughness_default: 0.5,
            prefer: PackPrefer::Prepacked,
            fill: FillSpec::default(),
        }
    );
    // Unset fills are left out of the JSON, so `profile_sha256` moves only when a fill is set.
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["packing"]["fill"], serde_json::json!({}));
}

/// DECISIONS Note M5: a fill is an 8-bit code per raw-map role.
#[test]
fn a_fill_is_an_8_bit_code_per_raw_map_role() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut doc = serde_json::to_value(
        reg.profile("cs-quality", Target::Cs, Subtype::Rmaos)
            .unwrap(),
    )
    .unwrap();
    merge_json(
        &mut doc,
        &to_patch("packing.fill.metallic", &serde_json::json!(255)),
    );
    let p = finish_profile(doc.clone(), Target::Cs, Subtype::Rmaos).unwrap();
    assert_eq!(p.packing.fill.get(RawMap::Metallic), Some(255));
    assert_eq!(p.packing.fill.get(RawMap::Ao), None);
    for bad in [
        serde_json::json!(256),
        serde_json::json!(-1),
        serde_json::json!(0.5),
    ] {
        let mut d = doc.clone();
        merge_json(&mut d, &to_patch("packing.fill.ao", &bad));
        assert!(
            finish_profile(d, Target::Cs, Subtype::Rmaos).is_err(),
            "{bad}"
        );
    }
}

/// DECISIONS Note M12: packing provenance round-trips and ranks above every existing source, so
/// `float_source_is_linear`'s `>= Suffix` comparison never sees it below a suffix fact.
#[test]
fn packing_provenance_round_trips_and_ranks_last() {
    for (p, s) in [
        (
            Provenance::Source("iron_roughness.png:R".into()),
            "source:iron_roughness.png:R",
        ),
        (
            Provenance::Source("1-iron_gloss.png:R".into()),
            "source:1-iron_gloss.png:R",
        ),
        (Provenance::Default("0.5".into()), "default:0.5"),
        (Provenance::Fill("255".into()), "fill:255"),
        (Provenance::Recipe("rmaos".into()), "recipe:rmaos"),
    ] {
        assert_eq!(p.to_string(), s);
        assert_eq!(s.parse::<Provenance>().unwrap(), p);
        assert_eq!(serde_json::to_value(&p).unwrap(), serde_json::json!(s));
        assert!(p > Provenance::Fallback && p > Provenance::Suffix(String::new()));
    }
}

#[test]
fn pack_codes_are_in_the_catalogue() {
    for (s, sev) in [
        ("PACK-DEFAULTED", Severity::Warning),
        ("PACK-SIZE-MISMATCH", Severity::Error),
        ("PACK-CONFLICT", Severity::Warning),
        ("PACK-UNUSED-INPUT", Severity::Warning),
    ] {
        let c = Code::parse(s).unwrap();
        assert_eq!((c.as_str(), c.severity()), (s, sev));
    }
}
