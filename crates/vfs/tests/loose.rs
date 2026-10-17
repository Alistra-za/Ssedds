#[test]
fn walks_case_insensitively_and_skips_facegen() {
    let d = tempfile::tempdir().unwrap();
    let mk = |p: &str| {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, b"x").unwrap();
    };
    mk("Meshes/Armor/A.NIF");
    mk("meshes/actors/character/FaceGenData/FaceGeom/skyrim.esm/0001.nif");
    mk("meshes/b.txt");
    mk("textures/a_d.dds");
    let (files, errs) = vfs::loose::walk(d.path(), &["meshes"], &["nif"]);
    assert!(errs.is_empty(), "{errs:?}");
    let got: Vec<_> = files.into_iter().map(|f| f.norm).collect();
    assert_eq!(got, ["meshes/armor/a.nif"]);
}

/// Directory link that needs no admin rights: a junction on Windows, a symlink elsewhere.
fn link_dir(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(windows)]
    {
        let st = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            // cmd reads `/` as a switch
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(st.status.success(), "mklink /J failed: {st:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn walk_follows_nested_junction() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    let target = d.path().join("target");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(root.join("meshes/b.nif"), b"x").unwrap();
    std::fs::write(target.join("a.nif"), b"x").unwrap();
    link_dir(&target, &root.join("meshes/sub"));
    let (got, errs) = vfs::loose::walk(&root, &["meshes"], &["nif"]);
    assert!(errs.is_empty(), "{errs:?}");
    let norms: Vec<_> = got.iter().map(|f| f.norm.as_str()).collect();
    assert_eq!(norms, ["meshes/b.nif", "meshes/sub/a.nif"]);
    assert!(got[1].abs.starts_with(root.join("meshes").join("sub")));
}

#[test]
fn walk_survives_a_link_cycle() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::create_dir_all(root.join("meshes/a")).unwrap();
    std::fs::write(root.join("meshes/a/x.nif"), b"x").unwrap();
    link_dir(&root.join("meshes"), &root.join("meshes/a/loop"));
    let (files, errs) = vfs::loose::walk(root, &["meshes"], &["nif"]);

    // walkdir reports the loop as exactly one "File system loop" error naming the link
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].0.ends_with("loop"), "{errs:?}");
    let norms: Vec<_> = files.into_iter().map(|f| f.norm).collect();
    assert_eq!(norms, ["meshes/a/x.nif"]);
}

#[test]
fn walk_reports_unreadable_root() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::write(&root, b"x").unwrap(); // a file, so read_dir fails with something other than NotFound
    let (files, errs) = vfs::loose::walk(&root, &["meshes"], &["nif"]);
    assert!(files.is_empty());
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].0, root.display().to_string());
}

#[test]
fn walk_missing_root_is_empty_not_error() {
    let d = tempfile::tempdir().unwrap();
    let (files, errs) = vfs::loose::walk(&d.path().join("nope"), &["meshes"], &["nif"]);
    assert!(files.is_empty());
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn walk_reports_a_broken_link_and_keeps_the_files() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    let target = d.path().join("gone");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(root.join("meshes/b.nif"), b"x").unwrap();
    link_dir(&target, &root.join("meshes/sub"));
    std::fs::remove_dir(&target).unwrap();
    let (files, errs) = vfs::loose::walk(&root, &["meshes"], &["nif"]);
    assert_eq!(files.len(), 1);
    assert_eq!(errs.len(), 1, "{errs:?}");
}

#[test]
fn walk_reports_a_broken_top_level_link() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    let target = d.path().join("gone");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    link_dir(&target, &root.join("meshes"));
    std::fs::remove_dir(&target).unwrap();
    let (files, errs) = vfs::loose::walk(&root, &["meshes"], &["nif"]);
    assert!(files.is_empty());
    assert_eq!(errs.len(), 1, "{errs:?}");
}
