//! Fix round 1 (M6 Track D): a preset file whose relative path is literally `builtin` or `inline` is still a file
//! source. This binary holds one test because it changes the process's current directory.
use engine::presets::*;
use std::path::Path;

#[test]
fn files_named_builtin_and_inline_are_file_sources() {
    let d = tempfile::tempdir().unwrap();
    std::env::set_current_dir(d.path()).unwrap();
    std::fs::write(
        "builtin",
        "schema_version = 1\nid = \"fromfile\"\nversion = \"1\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    std::fs::write(
        "inline",
        "schema_version = 1\nid = \"inl\"\nversion = \"1\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(r.add_file(Path::new("builtin")).unwrap(), "fromfile");
    assert!(!r.is_builtin("fromfile"));
    assert_eq!(r.source("fromfile"), Some("builtin"));
    // the same file twice loads once
    assert_eq!(r.add_file(Path::new("inline")).unwrap(), "inl");
    assert_eq!(r.add_file(Path::new("inline")).unwrap(), "inl");
    assert_eq!(
        r.user_presets(),
        vec![("fromfile", "builtin"), ("inl", "inline")]
    );
    // an inline preset is not the file named `inline`
    r.add_toml("schema_version = 1\nid = \"txt\"\nversion = \"1\"\nbase = \"cs-quality\"\n")
        .unwrap();
    assert_eq!(r.add_file(Path::new("inline")).unwrap(), "inl");
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
}
