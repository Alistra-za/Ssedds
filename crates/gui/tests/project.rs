use engine::codes::Code;
use engine::config::{RawMap, Target};
use sdc_gui::project::*;
use serde_json::json;

const HAND_WRITTEN: &str = r#"# My mod's settings — keep this comment.
preset = "cs-quality"

[defaults]
mips.levels = "full"   # trailing comment

[[rules]]
match = { glob = "**/foliage/**" }
overrides = { alpha = { policy = "cutout" } }
"#;

fn doc_with(text: &str) -> (tempfile::TempDir, ProjectDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc.toml"), text).unwrap();
    let doc = ProjectDoc::open(d.path());
    (d, doc)
}

#[test]
fn edits_keep_every_hand_written_byte() {
    let (d, mut doc) = doc_with(HAND_WRITTEN);
    doc.set_target(Target::Cpm);
    doc.set_file_value("landscape/rock01_n.png", "format", &json!("bc7"));
    doc.set_file_value("landscape/rock01_d.png", "alpha.policy", &json!("blend"));
    doc.set_allow(
        "landscape/rock01_n.png",
        &[Code::FmtBc5Normal].into_iter().collect(),
    );
    doc.set_fill(RawMap::Metallic, Some(255));
    assert!(doc.is_dirty());
    doc.save(false).unwrap();
    assert!(!doc.is_dirty());
    let text = std::fs::read_to_string(d.path().join("sdc.toml")).unwrap();
    for line in HAND_WRITTEN.lines() {
        assert!(text.contains(line), "lost line: {line}");
    }
    let cfg = ProjectDoc::open(d.path()).config().unwrap();
    assert_eq!(cfg.target, Some(Target::Cpm));
    assert_eq!(cfg.files["landscape/rock01_n.png"]["format"], "bc7");
    assert_eq!(
        cfg.files["landscape/rock01_n.png"]["allow"],
        json!(["FMT-BC5-NORMAL"])
    );
    assert_eq!(
        cfg.files["landscape/rock01_d.png"]["alpha"]["policy"],
        "blend"
    );
    assert_eq!(cfg.defaults["packing"]["fill"]["metallic"], 255);
    assert_eq!(cfg.rules.len(), 1);
}

/// Controller ruling (M6 Task 11): a `[files]` `reencode` opt-in is accepted and survives a GUI edit and save.
#[test]
fn a_reencode_entry_round_trips() {
    let text = "[files.\"landscape/old.dds\"]\nreencode = true\n";
    let (d, mut doc) = doc_with(text);
    assert!(!doc.read_only(), "{:?}", doc.error());
    assert_eq!(
        doc.file_value("landscape/old.dds", "reencode"),
        Some(json!(true))
    );
    doc.set_file_value("landscape/old.dds", "format", &json!("bc7"));
    doc.clear_file_value("landscape/old.dds", "format");
    doc.set_file_value("landscape/rock01.png", "format", &json!("bc1"));
    doc.save(false).unwrap();
    let back = ProjectDoc::open(d.path());
    assert!(!back.read_only(), "{:?}", back.error());
    assert_eq!(
        back.config().unwrap().files["landscape/old.dds"]["reencode"],
        true
    );
}

#[test]
fn awkward_keys_are_quoted_and_read_back() {
    let (d, mut doc) = doc_with("");
    let keys = [
        "a b/c d.png",
        "ü/naïve_n.png",
        "x.y/rock.01_rmaos",
        "[old]{1}*.png",
    ];
    for k in keys {
        doc.set_file_value(k, "alpha.threshold", &json!(90));
    }
    doc.save(false).unwrap();
    let cfg = ProjectDoc::open(d.path()).config().unwrap();
    for k in keys {
        assert_eq!(cfg.files[k]["alpha"]["threshold"], 90, "{k}");
    }
}

#[test]
fn clearing_removes_empty_tables_and_entries() {
    let (_d, mut doc) = doc_with("");
    doc.set_file_value("a.png", "alpha.policy", &json!("blend"));
    doc.set_file_value("a.png", "format", &json!("bc7"));
    doc.clear_file_value("a.png", "alpha.policy");
    assert_eq!(doc.file_value("a.png", "alpha.policy"), None);
    assert_eq!(doc.file_value("a.png", "format"), Some(json!("bc7")));
    assert!(!doc.config().unwrap().files["a.png"]
        .as_object()
        .unwrap()
        .contains_key("alpha"));
    doc.clear_file_value("a.png", "format");
    assert!(doc.file_keys().is_empty());
    doc.set_fill(RawMap::Ao, Some(10));
    doc.set_fill(RawMap::Ao, None);
    assert_eq!(doc.fill(RawMap::Ao), None);
}

#[test]
fn an_external_change_needs_force_and_an_invalid_file_is_read_only() {
    let (d, mut doc) = doc_with(HAND_WRITTEN);
    doc.set_target(Target::Vanilla);
    std::fs::write(
        d.path().join("sdc.toml"),
        "preset = \"other\"\n# edited elsewhere, longer text\n",
    )
    .unwrap();
    assert!(doc.changed_on_disk());
    assert_eq!(doc.save(false), Err(SaveError::ChangedOnDisk));
    doc.save(true).unwrap();
    assert!(!doc.changed_on_disk());

    let (_d, mut bad) = doc_with("target = [\n");
    assert!(bad.read_only());
    assert!(bad.error().unwrap().contains("sdc.toml"));
    bad.set_target(Target::Cs);
    assert!(!bad.is_dirty());
    assert_eq!(bad.save(true), Err(SaveError::ReadOnly));
}

#[test]
fn a_missing_file_is_created_on_save() {
    let d = tempfile::tempdir().unwrap();
    let mut doc = ProjectDoc::open(d.path());
    assert!(!doc.read_only());
    doc.set_file_value("a.png", "subtype", &json!("diffuse"));
    doc.save(false).unwrap();
    assert!(d.path().join("sdc.toml").exists());
}

#[test]
fn clearing_a_fill_leaves_no_empty_defaults_header() {
    let (d, mut doc) = doc_with("preset = \"x\"\n");
    doc.set_fill(RawMap::Ao, None);
    assert!(
        !doc.is_dirty(),
        "clearing a fill that is not there changes nothing"
    );
    doc.set_fill(RawMap::Ao, Some(10));
    doc.set_fill(RawMap::Ao, None);
    doc.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc.toml")).unwrap();
    assert!(!text.contains("[defaults]"), "{text}");
}

#[test]
fn an_inline_files_table_is_edited_not_a_panic() {
    let (d, mut doc) = doc_with("files = { \"a.png\" = { format = \"bc7\" } }\n");
    doc.set_file_value("a.png", "alpha.policy", &json!("blend"));
    doc.set_file_value("b.png", "format", &json!("bc1"));
    assert_eq!(doc.file_value("a.png", "format"), Some(json!("bc7")));
    assert_eq!(
        doc.file_value("a.png", "alpha.policy"),
        Some(json!("blend"))
    );
    doc.save(false).unwrap();
    let cfg = ProjectDoc::open(d.path()).config().unwrap();
    assert_eq!(cfg.files["a.png"]["alpha"]["policy"], "blend");
    assert_eq!(cfg.files["b.png"]["format"], "bc1");
    doc.clear_file_value("a.png", "alpha.policy");
    doc.clear_file_value("a.png", "format");
    assert_eq!(doc.file_keys(), vec!["b.png".to_string()]);
    doc.remove_file_entry("b.png");
    assert!(doc.file_keys().is_empty());
    assert!(doc.config().unwrap().files.is_empty());
}

#[test]
fn an_inline_entry_under_a_files_table_is_edited() {
    let (_d, mut doc) = doc_with("[files]\n\"a.png\" = { format = \"bc7\" }\n");
    doc.set_file_value("a.png", "subtype", &json!("diffuse"));
    doc.set_file_value("c.png", "format", &json!("bc3"));
    let files = doc.config().unwrap().files;
    assert_eq!(files["a.png"]["format"], "bc7");
    assert_eq!(files["a.png"]["subtype"], "diffuse");
    assert_eq!(files["c.png"]["format"], "bc3");
    doc.clear_file_value("a.png", "format");
    doc.remove_file_entry("c.png");
    let files = doc.config().unwrap().files;
    assert_eq!(files["a.png"], json!({"subtype": "diffuse"}));
    assert!(!files.contains_key("c.png"));
}

#[test]
fn an_inline_sub_table_is_edited_in_place() {
    let (_d, mut doc) = doc_with("[files.\"a.png\"]\nalpha = { policy = \"cutout\" }\n");
    doc.set_file_value("a.png", "alpha.threshold", &json!(90));
    assert_eq!(doc.file_value("a.png", "alpha.threshold"), Some(json!(90)));
    assert_eq!(
        doc.config().unwrap().files["a.png"]["alpha"]["threshold"],
        90
    );
    doc.clear_file_value("a.png", "alpha.policy");
    assert_eq!(doc.file_value("a.png", "alpha.policy"), None);
    let alpha = &doc.config().unwrap().files["a.png"]["alpha"];
    assert_eq!(*alpha, json!({"threshold": 90}));
    doc.clear_file_value("a.png", "alpha.threshold");
    assert!(doc.file_keys().is_empty());
}

#[test]
fn an_inline_defaults_packing_is_edited_in_place() {
    let (_d, mut doc) = doc_with("[defaults]\npacking = { fill = { ao = 10 } }\n");
    assert_eq!(doc.fill(RawMap::Ao), Some(10));
    doc.set_fill(RawMap::Metallic, Some(255));
    assert_eq!(doc.fill(RawMap::Metallic), Some(255));
    assert_eq!(
        doc.config().unwrap().defaults["packing"]["fill"]["metallic"],
        255
    );
    doc.set_fill(RawMap::Ao, None);
    assert_eq!(doc.fill(RawMap::Ao), None);
    let fill = &doc.config().unwrap().defaults["packing"]["fill"];
    assert_eq!(*fill, json!({"metallic": 255}));
}

#[test]
fn a_hand_written_key_is_found_case_insensitively() {
    let (d, mut doc) = doc_with("[files.\"Landscape/Rock01_N.png\"]\nformat = \"bc7\"\n");
    assert_eq!(
        doc.file_value("landscape/rock01_n.png", "format"),
        Some(json!("bc7"))
    );
    doc.set_file_value("landscape/rock01_n.png", "subtype", &json!("normal"));
    doc.set_allow(
        "landscape/rock01_n.png",
        &[Code::FmtBc5Normal].into_iter().collect(),
    );
    assert_eq!(doc.file_keys(), vec!["Landscape/Rock01_N.png".to_string()]);
    assert_eq!(doc.allow("LANDSCAPE/rock01_n.png").len(), 1);
    doc.save(false).unwrap();
    let files = ProjectDoc::open(d.path()).config().unwrap().files;
    assert_eq!(files.len(), 1);
    assert_eq!(files["Landscape/Rock01_N.png"]["subtype"], "normal");
    doc.remove_file_entry("landscape/ROCK01_N.png");
    assert!(doc.file_keys().is_empty());
}
