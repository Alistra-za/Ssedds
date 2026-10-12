//! 03 T-C1: the shader cache key names the bytes that were compiled, not where the folder is.
use render3d::compile::{compiler_id, program_key, ProgramKey};
use render3d::source::SourceSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn tree(root: &Path, a: &str) {
    fs::create_dir_all(root.join("Common")).unwrap();
    fs::write(
        root.join("Lighting.hlsl"),
        "#include \"Common/A.hlsli\"\nfloat4 main() : SV_Target { return A; }\n",
    )
    .unwrap();
    fs::write(
        root.join("Common/A.hlsli"),
        format!("static const float4 A = {a};\n"),
    )
    .unwrap();
}

fn src(root: &Path) -> Arc<render3d::source::ExpandedSource> {
    SourceSet::new(root.to_path_buf())
        .get(Path::new("Lighting.hlsl"))
        .unwrap()
}

fn key(root: &Path, defines: &[(&str, &str)]) -> ProgramKey {
    let d: Vec<(String, String)> = defines
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    program_key(&src(root), "main", "ps_5_0", &d)
}

#[test]
fn key_ignores_the_root_path() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    tree(a.path(), "1.0");
    tree(b.path(), "1.0");
    assert_ne!(a.path(), b.path());
    assert_eq!(src(a.path()).sha256, src(b.path()).sha256);
    assert_eq!(key(a.path(), &[("X", "1")]), key(b.path(), &[("X", "1")]));
    let shown = a.path().display().to_string().replace('\\', "/");
    assert!(
        !src(a.path()).text.contains(&shown),
        "the expansion names files relative to the root"
    );
    assert!(src(a.path()).text.contains("#line 1 \"Common/A.hlsli\""));
    assert_eq!(src(a.path()).files.len(), 2);
}

#[test]
fn editing_an_included_file_changes_the_key() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let before = key(d.path(), &[]);
    fs::write(
        d.path().join("Common/A.hlsli"),
        "static const float4 A = 0.5;\n",
    )
    .unwrap();
    assert_ne!(key(d.path(), &[]), before);
}

#[test]
fn define_order_does_not_matter_but_values_entry_and_target_do() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let ab = key(d.path(), &[("A", "1"), ("B", "2")]);
    assert_eq!(ab, key(d.path(), &[("B", "2"), ("A", "1")]));
    assert_ne!(ab, key(d.path(), &[("A", "1"), ("B", "3")]));
    assert_ne!(ab, key(d.path(), &[("A", "1")]));
    let s = src(d.path());
    let defs = vec![("A".to_string(), "1".to_string())];
    let ps = program_key(&s, "main", "ps_5_0", &defs);
    assert_ne!(ps, program_key(&s, "main", "vs_5_0", &defs));
    assert_ne!(ps, program_key(&s, "other", "ps_5_0", &defs));
    let pair = ProgramKey::pair(&ps, &program_key(&s, "main", "vs_5_0", &defs));
    assert_ne!(pair, ps);
    assert_eq!(pair.0.len(), 64);
}

#[test]
fn the_expansion_is_memoised_until_the_set_is_replaced() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let set = SourceSet::new(d.path().to_path_buf());
    let first = set.get(Path::new("Lighting.hlsl")).unwrap();
    fs::write(
        d.path().join("Common/A.hlsli"),
        "static const float4 A = 2.0;\n",
    )
    .unwrap();
    let again = set.get(Path::new("Lighting.hlsl")).unwrap();
    assert!(Arc::ptr_eq(&first, &again), "memo hit: no second expansion");
    let fresh = SourceSet::new(d.path().to_path_buf())
        .get(Path::new("Lighting.hlsl"))
        .unwrap();
    assert_ne!(fresh.sha256, first.sha256, "a new set sees the edit");
}

#[test]
fn a_missing_entry_file_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let e = SourceSet::new(d.path().to_path_buf()).get(Path::new("Nope.hlsl"));
    assert!(e.is_err());
}

/// The id is the loaded d3dcompiler_47.dll's file version; "unknown" means the query failed.
#[test]
fn compiler_id_is_the_loaded_dlls_file_version() {
    let id = compiler_id();
    assert_ne!(id, "unknown");
    let parts: Vec<&str> = id.split('.').collect();
    assert_eq!(parts.len(), 4, "{id}");
    assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{id}");
}
