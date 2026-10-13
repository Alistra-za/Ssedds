//! Audit 7 (DECISIONS Note Y7): the set views are built once per change, and follow every change.
mod support;
use sdc_gui::model::sets::views;
use std::sync::Arc;
use std::time::Duration;
use support::*;

#[test]
fn set_views_are_cached_until_an_input_changes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let a = views(&m);
    assert!(Arc::ptr_eq(&a, &views(&m)), "no change, no rebuild");
    // A document edit.
    m.rename_set("landscape/rock01", "landscape/stone").unwrap();
    assert!(!Arc::ptr_eq(&a, &views(&m)));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(views(&m).iter().any(|v| v.id == "landscape/stone"));
    // Undo restores the document without an edit_docs call.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!views(&m).iter().any(|v| v.id == "landscape/stone"));
    // The export folder.
    let before = views(&m);
    m.settings.export_folder = Some(d.path().join("elsewhere"));
    assert!(!Arc::ptr_eq(&before, &views(&m)));
    // An in-place write, announced.
    let before = views(&m);
    m.invalidate_views();
    assert!(!Arc::ptr_eq(&before, &views(&m)));
}

#[test]
fn every_sets_doc_edit_changes_its_revision() {
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::sets_doc::SetsDoc::open(d.path());
    let mut last = s.rev();
    let mut step = |s: &mut sdc_gui::sets_doc::SetsDoc, what: &str| {
        assert_ne!(s.rev(), last, "{what}");
        last = s.rev();
    };
    s.create_set("a/x");
    step(&mut s, "create_set");
    s.set_order(&["a/x".into()]);
    step(&mut s, "set_order");
    s.set_json_name(Some("n"));
    step(&mut s, "set_json_name");
    let t = s.text();
    s.restore(&t);
    step(&mut s, "restore");
    s.reload();
    step(&mut s, "reload");
    assert_ne!(
        sdc_gui::sets_doc::SetsDoc::open(d.path()).rev(),
        last,
        "a new document never reuses a revision"
    );
}
