use render3d::includes::expand;
use std::fs;

#[test]
fn expands_relative_to_root_and_includer() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    fs::create_dir_all(r.join("Common")).unwrap();
    fs::write(
        r.join("Lighting.hlsl"),
        "#include \"Common/A.hlsli\"\nfloat main_marker;\n",
    )
    .unwrap();
    fs::write(
        r.join("Common/A.hlsli"),
        "#include \"Common/B.hlsli\"\nfloat a;\n",
    )
    .unwrap();
    fs::write(r.join("Common/B.hlsli"), "float b;\n").unwrap();
    let e = expand(&r.join("Lighting.hlsl"), &[r.to_path_buf()]).unwrap();
    let ia = e.text.find("float a;").unwrap();
    let ib = e.text.find("float b;").unwrap();
    let im = e.text.find("float main_marker;").unwrap();
    assert!(ib < ia && ia < im);
    assert_eq!(e.files.len(), 3);
    assert!(e.text.contains("#line 1 \""));
}

#[test]
fn missing_include_left_for_compiler() {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("x.hlsl"),
        "#if 0\n#include \"Nope/Gone.hlsli\"\n#endif\nfloat x;\n",
    )
    .unwrap();
    let e = expand(&d.path().join("x.hlsl"), &[d.path().to_path_buf()]).unwrap();
    assert!(e.text.contains("#include \"Nope/Gone.hlsli\""));
}

#[test]
fn include_cycle_is_capped() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.hlsli"), "#include \"a.hlsli\"\n").unwrap();
    assert!(expand(&d.path().join("a.hlsli"), &[d.path().to_path_buf()]).is_err());
}

/// Real-CS check; skips cleanly when the folder is absent.
#[test]
#[ignore]
fn real_cs_lighting_expands() {
    let root = std::path::PathBuf::from(
        r"C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders",
    );
    let f = root.join("Lighting.hlsl");
    if !f.is_file() {
        eprintln!("skip: CS not present");
        return;
    }
    let e = expand(&f, std::slice::from_ref(&root)).unwrap();
    let left: Vec<&str> = e
        .text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with('#') && t[1..].trim_start().starts_with("include")
        })
        .collect();
    eprintln!(
        "files={} distinct={} bytes={} unexpanded={}",
        e.files.len(),
        e.files
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        e.text.len(),
        left.len()
    );
    for l in &left {
        eprintln!("UNEXPANDED: {l}");
    }
}

#[test]
fn spaced_and_tabbed_include_directives_expand() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    fs::write(
        r.join("m.hlsl"),
        "#\tinclude \"x.hlsli\"\n#  include <y.hlsli>\nfloat m;\n",
    )
    .unwrap();
    fs::write(r.join("x.hlsli"), "\u{feff}float x;\n").unwrap();
    fs::write(r.join("y.hlsli"), b"float y;\n\xff\n").unwrap();
    let e = expand(&r.join("m.hlsl"), &[r.to_path_buf()]).unwrap();
    assert_eq!(e.files.len(), 3);
    assert!(e.text.contains("float x;") && e.text.contains("float y;"));
    assert!(!e.text.contains('\u{feff}'));
    assert!(!e.text.contains("include"));
}
