use engine::codes::{Code, Severity};
use engine::config::*;
use engine::overrides::to_patch;
use engine::presets::*;
use serde_json::json;
use std::path::{Path, PathBuf};

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
    for t in [Target::Cs, Target::Vanilla] {
        assert_eq!(r.get(default_preset_for(t)).unwrap().target, Some(t));
    }
    assert_eq!(default_preset_for(Target::Cs), DEFAULT_PRESET);
    assert!(r.get(BASE_ID).unwrap().hidden);
    let ids: Vec<_> = r.selectable().iter().map(|p| p.id.clone()).collect();
    assert_eq!(
        ids,
        [
            "cs-compact",
            "cs-quality",
            "fast-preview",
            "vanilla-compact",
            "vanilla-quality"
        ]
    );
    assert_eq!(r.get(BASE_ID).unwrap().rules.len(), 2);
}

/// M6 Track D spec §2 (Notes T1–T3): the three new presets as profiles from presets alone (rules are not applied by
/// `profile`; the BC1 rules are checked by the goldens).
#[test]
fn compact_and_fast_preview_resolve_as_designed() {
    let r = PresetRegistry::builtin().unwrap();
    for (id, target) in [
        ("cs-compact", Some(Target::Cs)),
        ("vanilla-compact", Some(Target::Vanilla)),
        ("fast-preview", None),
    ] {
        let p = r.get(id).unwrap();
        assert_eq!(
            (p.schema_version, p.version.as_str(), p.hidden, p.target),
            (1, "1.0.0", false, target)
        );
        r.check_preset(id).unwrap();
    }
    // Owner rules (2026-10-12): data maps and colour that stays BC7 halve; pbr_emissive and hdr_sky keep full size.
    let halved = [
        Subtype::Normal,
        Subtype::LandscapeNormal,
        Subtype::PbrNormal,
        Subtype::Msn,
        Subtype::CoatNormal,
        Subtype::SkinWet,
        Subtype::Envmask,
        Subtype::Height,
        Subtype::MsnSpecular,
        Subtype::SkinSk,
        Subtype::ComplexMaterial,
        Subtype::Rmaos,
        Subtype::SkinRfaos,
        Subtype::Flow,
        Subtype::GenericTwoChannel,
        Subtype::GenericData,
        Subtype::Diffuse,
        Subtype::LandscapeDiffuse,
        Subtype::PbrAlbedo,
        Subtype::Glow,
        Subtype::PbrSubsurface,
        Subtype::PbrFuzz,
        Subtype::SubsurfaceTint,
        Subtype::BacklightTint,
        Subtype::UiSky,
        Subtype::SkinAlbedo,
    ];
    for st in Subtype::ALL {
        let p = r.profile("cs-compact", Target::Cs, st).unwrap();
        let want = if halved.contains(&st) {
            ResizeMode::Half
        } else {
            ResizeMode::None
        };
        assert_eq!(p.resize, want, "{st:?}");
        // Formats are the quality preset's: BC1 comes from the rules only.
        assert_eq!(
            p.format,
            r.profile("cs-quality", Target::Cs, st).unwrap().format,
            "{st:?}"
        );
    }
    let compact = r.get("cs-compact").unwrap();
    assert_eq!(compact.rules.len(), 2);
    assert_eq!(compact.rules[0].matcher.alpha, Some(AlphaContent::None));
    assert_eq!(compact.rules[0].matcher.subtypes.len(), 9);
    assert_eq!(
        compact.rules[1].matcher.alpha,
        Some(AlphaContent::Constant255)
    );
    assert_eq!(compact.rules[1].matcher.subtypes.len(), 7);
    for rule in &compact.rules {
        assert!(!rule.matcher.subtypes.contains(&Subtype::LandscapeDiffuse));
        assert!(!rule.matcher.subtypes.contains(&Subtype::PbrEmissive));
        assert_eq!(rule.overrides, json!({"format": "bc1", "resize": "none"}));
    }
    assert_eq!(r.get("vanilla-compact").unwrap().rules, compact.rules);
    for t in [Target::Cs, Target::Vanilla] {
        let p = r.profile("fast-preview", t, Subtype::Diffuse).unwrap();
        assert_eq!(p.encoder.effort, Effort::Fast);
        assert_eq!(p.validation.mips, ValidationMips::Smallest);
        assert_eq!(p.resize, ResizeMode::None);
    }
    for id in ["cs-compact", "vanilla-compact", "fast-preview"] {
        let t = r.get(id).unwrap().target.unwrap_or(Target::Cs);
        assert_eq!(
            r.profile(id, t, Subtype::Envmask)
                .unwrap()
                .encoder
                .bc45_search_radius,
            SearchRadius::Fast
        );
    }
    assert_eq!(
        r.profile("cs-quality", Target::Cs, Subtype::Envmask)
            .unwrap()
            .encoder
            .bc45_search_radius,
        SearchRadius::Default
    );
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
        (Provenance::ProjectFile, "project_file"),
        (Provenance::User, "user"),
        (Provenance::Fallback, "fallback"),
    ];
    for (p, s) in cases {
        assert_eq!(serde_json::to_value(&p).unwrap(), json!(s));
        assert_eq!(serde_json::from_value::<Provenance>(json!(s)).unwrap(), p);
    }
    assert!(serde_json::from_value::<Provenance>(json!("bogus")).is_err());
    assert!(Provenance::Builtin < Provenance::Preset("a".into()));
    assert!(
        Provenance::Suggestion < Provenance::ProjectFile
            && Provenance::ProjectFile < Provenance::User
    );
    // Spec §5.2's `set` (set-level decisions) is a value since M4.
    assert_eq!(
        serde_json::from_value::<Provenance>(json!("set")).unwrap(),
        Provenance::Set
    );
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
            recipe: Default::default(),
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
        ("PACK-RECIPE-CONFLICT", Severity::Warning),
        ("PACK-RECIPE-UNBUILDABLE", Severity::Warning),
        ("PACK-ORM-OVERRIDDEN", Severity::Info),
        ("SKIN-WET-FORM", Severity::Info),
    ] {
        let c = Code::parse(s).unwrap();
        assert_eq!((c.as_str(), c.severity()), (s, sev));
    }
}

const MINE: &str =
    "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

/// Carried fix 1 (M6 Track D, Note T7): a second preset under a loaded id is refused, naming both sources.
#[test]
fn duplicate_and_builtin_ids_are_refused_naming_both_sources() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(MINE).unwrap();
    let e = r.add_toml(MINE).unwrap_err().to_string();
    assert!(
        e.starts_with("CFG-INVALID") && e.contains("'mine'") && e.contains("inline"),
        "{e}"
    );
    let e = r
        .add_toml("schema_version = 1\nid = \"cs-quality\"\nversion = \"9\"\nbase = \"sdc-base\"\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("already loaded from builtin"), "{e}");
    assert!(r.is_builtin("cs-quality"));
    assert!(!r.is_builtin("mine"));
    assert_eq!(r.source("mine"), Some("inline"));
    assert_eq!(r.source("cs-quality"), Some("builtin"));
    assert_eq!(r.user_presets(), vec![("mine", "inline")]);
    assert_eq!(
        r.table("mine").unwrap()["base"].as_str(),
        Some("cs-quality")
    );
}

/// Review Focus 1: a Windows BOM and CRLF line endings load.
#[test]
fn a_preset_with_a_bom_and_crlf_loads() {
    let mut r = PresetRegistry::builtin().unwrap();
    let text = "\u{feff}schema_version = 1\r\nid = \"crlf\"\r\nversion = \"1.0.0\"\r\nbase = \"cs-quality\"\r\n[subtypes.diffuse]\r\nformat = \"bc1\"\r\n";
    assert_eq!(r.add_toml(text).unwrap(), "crlf");
    assert_eq!(
        r.profile("crlf", Target::Cs, Subtype::Diffuse)
            .unwrap()
            .format,
        Format::Bc1
    );
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "bom.toml",
        &text.replace("\"crlf\"", "\"bomfile\""),
    );
    assert!(r.add_dir(d.path()).is_empty());
    assert!(r.get("bomfile").is_some());
}

/// Spec §3 and Review Focus 2: everything loads first, then every user preset is checked; a bad one is skipped
/// with CFG-PRESET-SKIPPED, and so is a preset based on a skipped one.
#[test]
fn the_user_folder_loads_everything_then_checks_and_skips_bad_files() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    write(
        dir,
        "a_child.toml",
        "schema_version = 1\nid = \"child\"\nversion = \"1\"\nbase = \"zbase\"\n",
    );
    write(dir, "b_bad.toml", "this is not toml");
    write(
        dir,
        "c_dup.toml",
        "schema_version = 1\nid = \"cs-quality\"\nversion = \"1\"\nbase = \"sdc-base\"\n",
    );
    write(
        dir,
        "d_orphan.toml",
        "schema_version = 1\nid = \"orphan\"\nversion = \"1\"\nbase = \"missing\"\n",
    );
    write(dir, "e_badfield.toml", "schema_version = 1\nid = \"badfield\"\nversion = \"1\"\nbase = \"cs-quality\"\n[defaults]\nformat = \"bc9\"\n");
    write(
        dir,
        "f_on_orphan.toml",
        "schema_version = 1\nid = \"grandchild\"\nversion = \"1\"\nbase = \"orphan\"\n",
    );
    write(dir, "notes.txt", "not a preset");
    write(
        &dir.join("sub"),
        "deep.toml",
        "schema_version = 1\nid = \"deep\"\nversion = \"1\"\nbase = \"cs-quality\"\n",
    );
    write(dir, "z_base.toml", "schema_version = 1\nid = \"zbase\"\nversion = \"1\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n");
    let mut r = PresetRegistry::builtin().unwrap();
    let w = r.add_dir(dir);
    assert_eq!(
        r.profile("child", Target::Cs, Subtype::Diffuse)
            .unwrap()
            .format,
        Format::Bc1
    );
    let mut skipped: Vec<String> = w
        .iter()
        .map(|w| {
            assert_eq!(
                (w.code, w.severity),
                (Code::CfgPresetSkipped, Severity::Warning)
            );
            assert!(
                w.data["reason"].as_str().is_some_and(|s| !s.is_empty()),
                "{w:?}"
            );
            let file = w.data["file"].as_str().unwrap();
            file.rsplit(['/', '\\']).next().unwrap().to_string()
        })
        .collect();
    skipped.sort();
    assert_eq!(
        skipped,
        [
            "b_bad.toml",
            "c_dup.toml",
            "d_orphan.toml",
            "e_badfield.toml",
            "f_on_orphan.toml"
        ]
    );
    for id in ["orphan", "grandchild", "badfield", "deep"] {
        assert!(r.get(id).is_none(), "{id}");
    }
    assert!(r.is_builtin("cs-quality"));
    assert!(r.source("zbase").unwrap().ends_with("z_base.toml"));
    assert!(PresetRegistry::builtin()
        .unwrap()
        .add_dir(&dir.join("nope"))
        .is_empty());
}

/// Note T6: one file reached twice (the folder and --preset-file) loads once; a copy under the same id does not.
#[test]
fn the_same_file_twice_loads_once() {
    let d = tempfile::tempdir().unwrap();
    let p = write(d.path(), "mine.toml", MINE);
    let mut r = PresetRegistry::builtin().unwrap();
    assert!(r.add_dir(d.path()).is_empty());
    assert_eq!(r.add_file(&p).unwrap(), "mine");
    let copy = write(&d.path().join("copy"), "mine.toml", MINE);
    assert!(r
        .add_file(&copy)
        .unwrap_err()
        .to_string()
        .contains("already loaded from"));
}

#[test]
fn every_builtin_preset_passes_check_preset() {
    let r = PresetRegistry::builtin().unwrap();
    for p in r.selectable() {
        r.check_preset(&p.id).unwrap();
    }
    r.check_preset(BASE_ID).unwrap();
    let mut r = r;
    r.add_toml("schema_version = 1\nid = \"badrule\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { glob = \"**\" }\noverrides = { bogus = 1 }\n")
        .unwrap();
    assert!(r
        .check_preset("badrule")
        .unwrap_err()
        .to_string()
        .contains("bogus"));
}

/// Fix round 1: the reserved-id refusal names the incoming source and the built-in one.
#[test]
fn the_reserved_id_refusal_names_both_sources() {
    let base = "schema_version = 1\nid = \"sdc-base\"\nversion = \"1\"\n";
    let mut r = PresetRegistry::builtin().unwrap();
    let e = r.add_toml(base).unwrap_err().to_string();
    assert!(
        e.starts_with("CFG-INVALID")
            && e.contains("from inline")
            && e.contains("already loaded from builtin"),
        "{e}"
    );
    let d = tempfile::tempdir().unwrap();
    let p = write(d.path(), "base.toml", base);
    let e = r.add_file(&p).unwrap_err().to_string();
    assert!(
        e.contains(&format!("from {}", p.display())) && e.contains("already loaded from builtin"),
        "{e}"
    );
}

/// Fix round 1: a folder that cannot be read is one CFG-PRESET-SKIPPED naming it (a missing one is silent).
#[test]
fn an_unreadable_preset_folder_warns_naming_it() {
    let d = tempfile::tempdir().unwrap();
    let not_a_dir = write(d.path(), "file.toml", MINE);
    let w = PresetRegistry::builtin().unwrap().add_dir(&not_a_dir);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].code, Code::CfgPresetSkipped);
    assert_eq!(
        w[0].data["file"].as_str(),
        Some(not_a_dir.display().to_string().as_str())
    );
}

/// Fix round 1: a `*.toml` entry whose metadata cannot be read (a dangling link) is skipped with a warning naming it.
#[test]
fn a_dangling_preset_link_warns_naming_it() {
    let d = tempfile::tempdir().unwrap();
    let link = d.path().join("dangling.toml");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(d.path().join("gone.toml"), &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(d.path().join("gone.toml"), &link);
    if made.is_err() {
        eprintln!("skipped: cannot create a symlink here (Windows without Developer Mode)");
        return;
    }
    let w = PresetRegistry::builtin().unwrap().add_dir(d.path());
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].code, Code::CfgPresetSkipped);
    assert_eq!(
        w[0].data["file"].as_str(),
        Some(link.display().to_string().as_str())
    );
}

/// Spec §5: a config's schema_version is optional (1), a newer one is refused naming the file.
#[test]
fn config_schema_version_is_optional_and_a_newer_one_names_the_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc.toml");
    std::fs::write(&p, "schema_version = 1\npreset = \"cs-quality\"\n").unwrap();
    assert_eq!(ConfigFile::load(&p).unwrap().schema_version, Some(1));
    std::fs::write(&p, "schema_version = 2\n").unwrap();
    match ConfigFile::load(&p) {
        Err(e @ PresetError::NewerConfigSchema(_, 2)) => {
            assert!(e.to_string().contains("sdc.toml"), "{e}")
        }
        other => panic!("{other:?}"),
    }
    // Absent: not serialised back.
    let c = ConfigFile::parse("preset = \"cs-quality\"\n", "x").unwrap();
    assert!(serde_json::to_value(&c)
        .unwrap()
        .get("schema_version")
        .is_none());
}

/// Review Focus 1, config twin.
#[test]
fn a_config_with_a_bom_and_crlf_loads() {
    let c = ConfigFile::parse(
        "\u{feff}preset = \"cs-quality\"\r\n[defaults]\r\nformat = \"bc7\"\r\n",
        "user config",
    )
    .unwrap();
    assert_eq!(c.preset.as_deref(), Some("cs-quality"));
}

#[test]
fn a_newer_preset_is_still_newer_schema() {
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(
        r.add_toml("schema_version = 3\nid = \"x\"\n"),
        Err(PresetError::NewerSchema(3))
    );
    assert!(matches!(
        r.add_toml("id = \"x\"\nversion = \"1\"\n"),
        Err(PresetError::Parse(_))
    ));
}

/// Carried fix 2 (Note T10): a null patch neither erases nor adds a key.
#[test]
fn null_patches_never_add_absent_keys() {
    let mut a = json!({"format": "bc7"});
    merge_json(
        &mut a,
        &json!({"header": null, "alpha": {"policy": null, "threshold": 5}, "format": null}),
    );
    assert_eq!(a, json!({"format": "bc7", "alpha": {"threshold": 5}}));
    // A patch object replacing a null or scalar base is filtered the same way.
    for base in [json!({"alpha": null}), json!({"alpha": 5})] {
        let mut a = base;
        merge_json(&mut a, &json!({"alpha": {"policy": null, "threshold": 5}}));
        assert_eq!(a, json!({"alpha": {"threshold": 5}}));
    }
}

/// Note T2: `match.subtypes` round-trips and is not serialised when empty (existing preset hashes hold).
#[test]
fn rule_match_subtypes_is_optional() {
    let r: Rule = toml::from_str("match = { alpha = \"none\", subtypes = [\"diffuse\", \"glow\"] }\noverrides = { format = \"bc1\" }\n").unwrap();
    assert_eq!(r.matcher.subtypes, vec![Subtype::Diffuse, Subtype::Glow]);
    let plain = serde_json::to_value(RuleMatch::default()).unwrap();
    assert!(plain.get("subtypes").is_none());
}

const ULTRA_ALIASES: &str = "schema_version = 1\nid = \"ultra\"\nversion = \"2.0.0\"\nbase = \"cs-compact\"\n[subtypes.diffuse]\nalpha = { threshold = 100 }\n[suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n";

/// Spec §4 and Review Focus 4: an export keeps its `base` link and aliases, validates where the original's base is loaded,
/// and resolves equal to the original for every sub-type.
#[test]
fn export_round_trips_and_keeps_aliases() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(ULTRA_ALIASES).unwrap();
    let text = r.export("ultra", "ultra-export").unwrap();
    let out: PresetFile = toml::from_str(&text).unwrap();
    assert_eq!(out.id, "ultra-export");
    assert_eq!(out.base.as_deref(), Some("cs-compact"));
    assert_eq!(
        out.suffix_aliases.get("rmaosultrahigh").map(String::as_str),
        Some("rmaos")
    );
    // Validate beside the built-ins only: the original's alias would conflict (Note T8).
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "export.toml", &text);
    let clean = PresetRegistry::builtin().unwrap();
    assert_eq!(clean.validate_file(&f), Vec::<String>::new());
    let mut both = PresetRegistry::builtin().unwrap();
    let id = both.add_file(&f).unwrap();
    for st in Subtype::ALL {
        assert_eq!(
            both.profile(&id, Target::Cs, st).unwrap(),
            r.profile("ultra", Target::Cs, st).unwrap(),
            "{st:?}"
        );
    }
}

/// Spec §4: the default export is the preset's own table (base stays a link) under the new id.
#[test]
fn default_export_keeps_the_base_link_under_a_new_id() {
    let r = PresetRegistry::builtin().unwrap();
    let text = r.export("cs-compact", "my-compact").unwrap();
    let p: PresetFile = toml::from_str(&text).unwrap();
    assert_eq!(
        (p.id.as_str(), p.base.as_deref()),
        ("my-compact", Some("cs-quality"))
    );
    assert_eq!(p.rules, r.get("cs-compact").unwrap().rules);
    assert!(matches!(
        r.export("nope", "x"),
        Err(PresetError::Unknown(_))
    ));
}

/// Spec §4: validate names the first problem; a folder file validates in place of its own copy.
#[test]
fn validate_file_reports_problems_and_accepts_a_folder_file() {
    let d = tempfile::tempdir().unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    let mine = write(d.path(), "mine.toml", MINE);
    assert!(r.add_dir(d.path()).is_empty());
    assert!(
        r.validate_file(&mine).is_empty(),
        "a folder file validates against itself"
    );
    for (name, text, says) in [
        ("dup.toml", "schema_version = 1\nid = \"cs-quality\"\nversion = \"1\"\n", "already loaded"),
        ("nobase.toml", "schema_version = 1\nid = \"x\"\nversion = \"1\"\nbase = \"nope\"\n", "nope"),
        ("unknown.toml", "schema_version = 1\nid = \"y\"\nversion = \"1\"\nbogus = 1\n", "bogus"),
        ("newer.toml", "schema_version = 9\nid = \"z\"\n", "newer"),
        ("rule.toml", "schema_version = 1\nid = \"w\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { subtypes = [\"nope\"] }\n", "nope"),
    ] {
        let f = write(&d.path().join("v"), name, text);
        let p = r.validate_file(&f);
        assert_eq!(p.len(), 1, "{name}: {p:?}");
        assert!(p[0].contains(says), "{name}: {p:?}");
    }
}

/// Fix round 1: an export under an id that is already loaded is refused, naming that preset's source.
#[test]
fn export_refuses_a_loaded_new_id() {
    let r = PresetRegistry::builtin().unwrap();
    let e = r
        .export("cs-compact", "cs-quality")
        .unwrap_err()
        .to_string();
    assert!(e.contains("already loaded") && e.contains("builtin"), "{e}");
}

/// Note AT: vanilla-compact halves a complex `_m` (BC7 with alpha), as cs-compact does.
#[test]
fn vanilla_compact_halves_a_complex_m() {
    let r = PresetRegistry::builtin().unwrap();
    let p = r
        .profile("vanilla-compact", Target::Vanilla, Subtype::ComplexMaterial)
        .unwrap();
    assert_eq!(
        (p.format, p.resize, p.alpha.policy),
        (Format::Bc7, ResizeMode::Half, AlphaPolicy::Data)
    );
}

/// Note AZ: the cpm target and its preset are gone, with no alias: `target = "cpm"` and `base = "cpm-quality"` are
/// errors.
#[test]
fn cpm_target_and_preset_are_rejected() {
    let mut r = PresetRegistry::builtin().unwrap();
    assert!(r.get("cpm-quality").is_none());
    let e = r
        .add_toml("schema_version = 1\nid = \"a\"\nversion = \"1\"\nbase = \"sdc-base\"\ntarget = \"cpm\"\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("cpm"), "{e}");
    let added = r
        .add_toml("schema_version = 1\nid = \"b\"\nversion = \"1\"\nbase = \"cpm-quality\"\n")
        .and_then(|id| r.chain(&id).map(|_| ()));
    assert!(added.unwrap_err().to_string().contains("cpm-quality"));
    assert!(toml::from_str::<engine::presets::ConfigFile>("target = \"cpm\"\n").is_err());
}

/// Note AZ1 (review M3): a user preset with `target = "cpm"` in the presets folder is skipped with CFG-PRESET-SKIPPED
/// (Note T6, unchanged), and the warning names the unknown target.
#[test]
fn a_user_preset_with_the_cpm_target_is_skipped_naming_it() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("old.toml"),
        "schema_version = 1\nid = \"old\"\nversion = \"1\"\nbase = \"sdc-base\"\ntarget = \"cpm\"\n",
    )
    .unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    let w = r.add_dir(d.path());
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].code, engine::codes::Code::CfgPresetSkipped);
    assert!(w[0].message.contains("`cpm`"), "{}", w[0].message);
    assert!(r.get("old").is_none());
}

/// Audit REPORT #7 (Note BC): a preset rule whose glob does not compile fails `check_preset`.
#[test]
fn a_rule_glob_that_does_not_compile_fails_check_preset() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml("schema_version = 1\nid = \"badglob\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { glob = \"**/{a,b\" }\noverrides = { mips = { levels = 2 } }\n")
        .unwrap();
    let e = r.check_preset("badglob").unwrap_err().to_string();
    assert!(e.contains("glob"), "{e}");
}

/// Audit EC-4 (Note BC): `overrides.subtype` on a rule would be overwritten by the decided sub-type, so it is refused.
#[test]
fn a_rule_override_of_the_subtype_fails_check_preset() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml("schema_version = 1\nid = \"stover\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { suffix = \"n\" }\noverrides = { subtype = \"pbr_normal\" }\n")
        .unwrap();
    let e = r.check_preset("stover").unwrap_err().to_string();
    assert!(e.contains("not in overrides"), "{e}");
}

/// Trace-debug #15 (Note BQ): a file refused because a preset loaded before it claimed its alias or id is retried
/// once that preset is skipped as broken, so the valid one loads.
#[test]
fn user_dir_retries_file_refused_by_a_skipped_preset() {
    for (a, b, id) in [
        (
            "schema_version = 1\nid = \"mine-old\"\nversion = \"1\"\nbase = \"cs-qualty\"\n[suffix_aliases]\nfoo = \"s\"\n",
            "schema_version = 1\nid = \"mine\"\nversion = \"1\"\nbase = \"cs-quality\"\n[suffix_aliases]\nfoo = \"s\"\n",
            "mine",
        ),
        (
            "schema_version = 1\nid = \"same\"\nversion = \"1\"\nbase = \"cs-qualty\"\n",
            "schema_version = 1\nid = \"same\"\nversion = \"2\"\nbase = \"cs-quality\"\n",
            "same",
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.toml"), a).unwrap();
        std::fs::write(d.path().join("b.toml"), b).unwrap();
        let mut r = PresetRegistry::builtin().unwrap();
        let w = r.add_dir(d.path());
        assert!(
            r.source(id).is_some_and(|s| s.ends_with("b.toml")),
            "{id} not loaded from b.toml: {w:?}"
        );
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].message.contains("a.toml"), "{}", w[0].message);
    }
}
