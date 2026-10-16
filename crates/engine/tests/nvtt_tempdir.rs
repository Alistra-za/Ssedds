//! Audit EP-7 (Note BC): an NVTT temp folder left by a crashed run with a recycled pid is never reused. Its own test
//! binary, so no other test in this process holds a `sdc-nvtt-<pid>-<n>` folder.
use engine::nvtt::TempDir;

#[test]
fn a_leftover_temp_folder_is_skipped() {
    let pid = std::process::id();
    let stale: Vec<_> = (0..8)
        .map(|n| std::env::temp_dir().join(format!("sdc-nvtt-{pid}-{n}")))
        .collect();
    for d in &stale {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("l0.out.dds"), b"stale").unwrap();
    }
    let t = TempDir::new().unwrap();
    let fresh = !stale.contains(&t.path().to_path_buf());
    let empty = std::fs::read_dir(t.path()).unwrap().next().is_none();
    drop(t);
    for d in &stale {
        let _ = std::fs::remove_dir_all(d);
    }
    assert!(fresh && empty, "reused a leftover folder");
}
