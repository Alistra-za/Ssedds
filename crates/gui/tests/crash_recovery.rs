//! Note AR: a main-thread panic writes unsaved edits to `<file>.recovery` through the real hook; a worker's panic
//! does not; a save removes it (the next open restores it: `recovery.rs`, Note FB5).
//! One test: the panic hook and the snapshot are process-wide.
mod support;
use sdc_gui::crash_log;
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

    // Note FB5: the file is no banner; the next open restores it (`tests/recovery.rs`).
    m.refresh_banner();
    assert!(!m.banner.as_deref().unwrap_or("").contains(".recovery"));

    m.save().unwrap();
    assert!(!rec.exists(), "a save removes the recovery file");
}

/// Trace-debug wave 2 #3: an automatic save (the export folder, a JSON hash record) of a document the user never
/// saved does not hold the recovered edits, so it keeps the unrestored `.recovery` file.
#[test]
fn an_automatic_save_keeps_an_unrestored_recovery_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let path = m.sets_doc.as_ref().unwrap().path().to_path_buf();
    let rec = crash_log::recovery_path(&path);
    std::fs::write(&rec, "# edits from a crash\n").unwrap();
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    m.set_export_folder(d.path().join("elsewhere"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("elsewhere"),
        "the export folder was saved"
    );
    assert_eq!(
        std::fs::read_to_string(&rec).unwrap(),
        "# edits from a crash\n",
        "the recovery file is kept"
    );
}
