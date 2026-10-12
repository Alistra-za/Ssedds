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
