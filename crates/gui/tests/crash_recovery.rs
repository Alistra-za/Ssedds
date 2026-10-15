//! Note AR: a main-thread panic writes unsaved edits to `<file>.recovery` through the real hook; a worker's panic
//! does not; the next open says so while the recovery file is newer; a save removes it.
//! One test: the panic hook and the snapshot are process-wide.
mod support;
use sdc_gui::crash_log;
use std::time::{Duration, SystemTime};
use support::*;

fn panic_on(thread: &str) {
    let r = std::thread::Builder::new()
        .name(thread.into())
        .spawn(|| panic!("test panic"))
        .unwrap()
        .join();
    assert!(r.is_err());
}

#[test]
fn a_main_thread_panic_keeps_unsaved_edits_beside_the_untouched_original() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    m.save().unwrap();
    m.set_target(engine::config::Target::Cs);
    assert!(m.is_dirty());
    let path = m.project.as_ref().unwrap().path().to_path_buf();
    let text = m.project.as_ref().unwrap().text();
    let before = std::fs::read(&path).unwrap();
    let rec = crash_log::recovery_path(&path);

    let logs = tempfile::tempdir().unwrap();
    crash_log::install_in(Some(logs.path().to_path_buf()));
    crash_log::set_recovery(m.recovery_snapshot());

    panic_on("worker");
    assert!(
        !rec.exists(),
        "a caught worker panic writes no recovery file"
    );

    panic_on("main");
    assert_eq!(std::fs::read_to_string(&rec).unwrap(), text);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the original is untouched"
    );
    assert!(
        std::fs::read_dir(logs.path()).unwrap().count() > 0,
        "crash log written"
    );

    let name = |p: &std::path::Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let note = format!(
        "Unsaved edits from a crash were kept in {}: rename it over {} to restore, or delete it.",
        name(&rec),
        name(&path)
    );
    m.refresh_banner();
    assert!(
        m.banner.as_deref().unwrap_or("").contains(&note),
        "{:?}",
        m.banner
    );

    // A recovery file older than its document is ignored.
    std::fs::File::options()
        .write(true)
        .open(&rec)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))
        .unwrap();
    m.refresh_banner();
    assert!(!m.banner.as_deref().unwrap_or("").contains("from a crash"));

    m.save().unwrap();
    assert!(!rec.exists(), "a save removes the recovery file");
}
