use engine::config::Subtype;
use engine::discover::{InputFile, InputKind};
use engine::overrides::*;
use serde_json::json;
use std::path::{Path, PathBuf};

#[test]
fn parses_scopes_and_typed_values() {
    let o = parse_set("subtype:rmaos.format=bc1").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str(), o.value),
        (Scope::Subtype(Subtype::Rmaos), "format", json!("bc1"))
    );
    let o = parse_set("file:*.png.alpha.policy=blend").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str()),
        (Scope::File("*.png".into()), "alpha.policy")
    );
    let o = parse_set("file:**/wood01_*.png.alpha.threshold=100").unwrap();
    assert_eq!((o.field.as_str(), o.value), ("alpha.threshold", json!(100)));
    let o = parse_set("alpha.exact_opaque=true").unwrap();
    assert_eq!((o.scope, o.value), (Scope::All, json!(true)));
    assert_eq!(parse_set("mips.levels=none").unwrap().value, json!("none"));
    assert!(parse_set("colour.tag=srgb").is_err());
    assert!(parse_set("subtype:nope.format=bc7").is_err());
    assert!(parse_set("format").is_err());
    assert_eq!(
        to_patch("alpha.policy", &json!("data")),
        json!({"alpha": {"policy": "data"}})
    );
}

#[test]
fn subtype_values_are_checked() {
    assert_eq!(
        parse_set("file:*_n.png.subtype=normal").unwrap().value,
        json!("normal")
    );
    assert!(parse_set("subtype=nrmal").is_err());
    assert!(parse_set("file:*.png.subtype=nope").is_err());
    // a sub-type-scoped sub-type is meaningless
    assert!(parse_set("subtype:normal.subtype=msn").is_err());
}

/// M2.5: every packing field is settable, unscoped, per sub-type and per (packed) file name.
#[test]
fn packing_fields_are_settable() {
    for s in [
        "packing.enabled=false",
        "packing.roughness_default=0.6",
        "packing.prefer=raw",
        "packing.fill.metallic=255",
        "subtype:rmaos.packing.fill.ao=200",
        "file:**/iron_rmaos.png.packing.fill.specular=128",
    ] {
        assert!(parse_set(s).is_ok(), "{s}");
    }
    let o = parse_set("file:**/iron_rmaos.png.packing.fill.specular=128").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str(), o.value),
        (
            Scope::File("**/iron_rmaos.png".into()),
            "packing.fill.specular",
            json!(128)
        )
    );
    // Role ids only, not exporter terms.
    assert!(parse_set("packing.fill.metal=1").is_err());
}

#[test]
fn path_keys_match_exactly_or_without_the_last_extension() {
    assert!(path_key_matches(
        "landscape/rock01_n.png",
        "landscape/rock01_n.png"
    ));
    assert!(path_key_matches(
        "Landscape/Rock01_N.PNG",
        "landscape/rock01_n.png"
    ));
    // A packed output is keyed without extension; its virtual name takes its first source's extension.
    assert!(path_key_matches(
        "landscape/rock01_rmaos",
        "landscape/rock01_rmaos.tga"
    ));
    // Glob syntax is literal.
    assert!(path_key_matches("a/[old]{1}*.png", "a/[old]{1}*.png"));
    assert!(!path_key_matches("a/[old]{1}*.png", "a/o1x.png"));
    // No basename matching across folders.
    assert!(!path_key_matches("rock01_n.png", "landscape/rock01_n.png"));
    assert!(!path_key_matches(
        "landscape/rock01_n.png",
        "other/landscape/rock01_n.png"
    ));
    // Only the last extension is dropped, and only from the file.
    assert!(path_key_matches("a/rock.01_rmaos", "a/rock.01_rmaos.png"));
    assert!(!path_key_matches("a/rock", "a/rock.01_rmaos.png"));
}

#[test]
fn project_rel_uses_the_virtual_file_name() {
    let root = Path::new("C:/mod/textures");
    let real = InputFile {
        path: PathBuf::from("C:/mod/textures/landscape/rock01_roughness.png"),
        rel: PathBuf::from("landscape/rock01_roughness.png"),
        game_path: "textures/landscape/rock01_roughness.png".into(),
        output: PathBuf::from("C:/out/landscape/rock01_roughness.dds"),
        kind: InputKind::Image,
    };
    assert_eq!(
        project_rel(root, &real).as_deref(),
        Some("landscape/rock01_roughness.png")
    );
    let virt = InputFile {
        rel: PathBuf::from("landscape/rock01_rmaos.png"),
        ..real.clone()
    };
    assert_eq!(
        project_rel(root, &virt).as_deref(),
        Some("landscape/rock01_rmaos.png")
    );
    assert_eq!(project_rel(Path::new("D:/elsewhere"), &real), None);
}

#[test]
fn project_rel_keeps_the_folder_case_and_ignores_the_root_case() {
    let input = InputFile {
        path: PathBuf::from(r"C:\Mod\Textures\Landscape\Rock01_N.png"),
        rel: PathBuf::from(r"Landscape\Rock01_N.png"),
        game_path: "textures/landscape/rock01_n.png".into(),
        output: PathBuf::from("C:/out/Landscape/Rock01_N.dds"),
        kind: InputKind::Image,
    };
    assert_eq!(
        project_rel(Path::new("c:/mod/textures"), &input).as_deref(),
        Some("Landscape/Rock01_N.png")
    );
}

#[test]
fn reencode_is_a_boolean_file_or_batch_field_never_subtype_scoped() {
    assert!(parse_set("reencode=true").is_ok());
    assert!(parse_set("file:**/old/*.dds.reencode=true").is_ok());
    let e = parse_set("subtype:diffuse.reencode=true").unwrap_err();
    assert!(e.contains("before the sub-type"), "{e}");
    let e = parse_set("reencode=yes").unwrap_err();
    assert!(e.contains("true or false"), "{e}");
    assert!(FIELD_PATHS.contains(&REENCODE_FIELD));
}
