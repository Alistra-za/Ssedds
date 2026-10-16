//! Audit 2026-10-15 GUI data items 11 and 22.
mod support;
use support::*;

/// Item 11: the recovery snapshot is rebuilt only when a document changed.
#[test]
fn the_recovery_snapshot_is_rebuilt_only_after_an_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.recovery_changed().is_some(), "the first frame sets it");
    assert!(m.recovery_changed().is_none(), "nothing changed");
    m.project
        .as_mut()
        .unwrap()
        .set_target(engine::config::Target::Vanilla);
    let snap = m.recovery_changed().expect("an edit rebuilds it");
    assert_eq!(snap.len(), 1);
    assert!(m.recovery_changed().is_none());
}

/// Item 22: opening a folder forgets the previous folder's file checks.
#[test]
fn opening_a_folder_clears_the_stat_cache() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let now = std::time::Instant::now();
    let p = d.path().join("x");
    m.stats.borrow_mut().is_file(&p, now);
    let before = m.stats.borrow().stats;
    m.open_folder(m.root.clone().unwrap());
    m.stats.borrow_mut().is_file(&p, now);
    assert_eq!(m.stats.borrow().stats, before + 1, "asked again");
}
