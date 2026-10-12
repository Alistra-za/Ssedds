use engine::schemas::*;
use std::path::Path;

fn schema_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

/// A type change without a schema update fails here: the committed files must be exactly what the types
/// generate, byte for byte (`.gitattributes` pins `schema/*.json` to LF so a Windows checkout matches too).
#[test]
fn committed_schemas_match_the_types() {
    for kind in SchemaKind::FILES {
        let path = schema_dir().join(kind.file_name());
        let committed = std::fs::read(&path).unwrap_or_default();
        assert!(
            committed == schema_json(kind).as_bytes(),
            "schema/{} is stale: run `sdc presets schema all --out schema`",
            kind.file_name()
        );
    }
}

#[test]
fn schema_dir_holds_exactly_the_published_files() {
    let mut found: Vec<String> = std::fs::read_dir(schema_dir())
        .expect("schema/ exists")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = SchemaKind::FILES.iter().map(|k| k.file_name()).collect();
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn schema_json_is_deterministic_lf_and_newline_terminated() {
    for kind in SchemaKind::FILES {
        let a = schema_json(kind);
        assert_eq!(a, schema_json(kind));
        assert!(a.ends_with("}\n") && !a.contains('\r'), "{kind}");
        let v: serde_json::Value = serde_json::from_str(&a).unwrap();
        assert!(
            v.get("$schema").is_some() || v.get("title").is_some(),
            "{kind}"
        );
    }
}

#[test]
fn write_all_writes_every_file_and_names_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    write_all(dir.path()).unwrap();
    for kind in SchemaKind::FILES {
        let text = std::fs::read_to_string(dir.path().join(kind.file_name())).unwrap();
        assert_eq!(text, schema_json(kind));
        assert_eq!(kind.name().parse::<SchemaKind>(), Ok(kind));
        assert_eq!(kind.to_string(), kind.name());
    }
    assert_eq!("all".parse::<SchemaKind>(), Ok(SchemaKind::All));
    assert!("nope".parse::<SchemaKind>().is_err());
}

/// The shapes added in review fixes are in the schemas (the verbs' reports are checked against their types in
/// the CLI tests).
#[test]
fn schemas_cover_the_reviewed_fields() {
    let event = schema_json(SchemaKind::Event);
    for needle in ["skipped_intentional", "file_skipped", "batch_done"] {
        assert!(event.contains(needle), "event schema lacks {needle}");
    }
    let manifest = schema_json(SchemaKind::Manifest);
    for needle in ["sources", "bc1_model", "alias_chain"] {
        assert!(manifest.contains(needle), "manifest schema lacks {needle}");
    }
    assert!(schema_json(SchemaKind::Preset).contains("suffix_aliases"));
    let info = schema_json(SchemaKind::DdsInfo);
    assert!(info.contains("dds_info") && info.contains("header_issues"));
    assert!(schema_json(SchemaKind::DdsValidation).contains("dds_validation"));
}

/// Final review Minor 2: `schema_version = 0` is refused by the migration hook, so the config schema says so.
#[test]
fn config_schema_version_minimum_is_one() {
    let v: serde_json::Value = serde_json::from_str(&schema_json(SchemaKind::Config)).unwrap();
    let sv = &v["properties"]["schema_version"];
    assert_eq!(sv["minimum"], 1, "{sv}");
}
