mod support;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;

fn doc(text: Option<&str>) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    if let Some(t) = text {
        std::fs::write(d.path().join("sdc-project.toml"), t).unwrap();
    }
    let s = SetsDoc::open(d.path());
    (d, s)
}

#[test]
fn a_missing_file_is_an_empty_clean_document() {
    let (_d, s) = doc(None);
    assert!(!s.is_dirty() && !s.read_only());
    assert!(s.entries().is_empty());
    assert_eq!(s.json_name(), None);
}

#[test]
fn entries_read_back_with_their_settings_flattened() {
    let (_d, s) = doc(Some(
        "[project]\njson_name = \"pack\"\norder = [\"b/two\", \"a/one\"]\n\n[defaults]\nspecular_level = 0.04\n\n\
         [[sets]]\nid = \"a/one\"\ncreated = true\nadd = [\"x/y.png\"]\nborrow = { slot6 = \"b/two\" }\n\
         parallax = true\nglint.screen_space_scale = 1.5\nemissive_color = [255, 128, 0]\n",
    ));
    assert_eq!(s.json_name().as_deref(), Some("pack"));
    assert_eq!(s.order(), vec!["b/two".to_string(), "a/one".into()]);
    let e = &s.entries()[0];
    assert_eq!(e.id, "a/one");
    assert!(e.created);
    assert_eq!(e.add, vec!["x/y.png".to_string()]);
    assert_eq!(e.borrow.get(&6).map(String::as_str), Some("b/two"));
    assert_eq!(e.settings["parallax"], json!(true));
    assert_eq!(e.settings["glint.screen_space_scale"], json!(1.5));
    assert_eq!(e.settings["emissive_color"], json!([255, 128, 0]));
    assert!(!e.settings.contains_key("id") && !e.settings.contains_key("add"));
}

#[test]
fn edits_preserve_comments_and_round_trip() {
    let (d, mut s) = doc(Some(
        "# my notes\n[[sets]]\nid = \"a/one\" # keep me\nparallax = true\n",
    ));
    s.set_value("a/one", "displacement_scale", Some(json!(0.4)));
    s.set_value("a/one", "glint.screen_space_scale", Some(json!(1.5)));
    s.set_value("b/two", "hair", Some(json!(true))); // creates an entry
    s.set_value("a/one", "parallax", None); // removes the key
    s.add_member("a/one", "x/y.png");
    s.set_borrow("a/one", 6, Some("b/two"));
    s.set_json_name(Some("pack"));
    assert!(s.is_dirty());
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(
        text.contains("# my notes") && text.contains("# keep me"),
        "{text}"
    );
    let again = SetsDoc::open(d.path());
    let a = again
        .entries()
        .into_iter()
        .find(|e| e.id == "a/one")
        .unwrap();
    assert_eq!(a.settings["displacement_scale"], json!(0.4));
    assert_eq!(a.settings["glint.screen_space_scale"], json!(1.5));
    assert!(!a.settings.contains_key("parallax"));
    assert_eq!(a.add, vec!["x/y.png".to_string()]);
    assert_eq!(a.borrow.get(&6).map(String::as_str), Some("b/two"));
    assert!(again.entries().iter().any(|e| e.id == "b/two"));
}

#[test]
fn create_rename_delete_and_order() {
    let (_d, mut s) = doc(None);
    s.create_set("mine/one");
    s.create_set("mine/two");
    s.rename_set("mine/one", "mine/uno");
    assert!(s
        .entries()
        .iter()
        .any(|e| e.id == "mine/uno" && e.created && e.renamed_from.is_none()));
    s.set_order(&["mine/two".to_string(), "mine/uno".into()]);
    assert_eq!(s.order(), vec!["mine/two".to_string(), "mine/uno".into()]);
    s.delete_set("mine/two");
    assert!(!s.entries().iter().any(|e| e.id == "mine/two"));
    assert_eq!(
        s.order(),
        vec!["mine/uno".to_string()],
        "a deleted set leaves the order list"
    );
    // Renaming an automatic set (no entry yet) records where it came from.
    s.rename_set("landscape/rock01", "landscape/boulder");
    let b = s
        .entries()
        .into_iter()
        .find(|e| e.id == "landscape/boulder")
        .unwrap();
    assert_eq!(b.renamed_from.as_deref(), Some("landscape/rock01"));
    assert!(!b.created);
}

#[test]
fn a_bad_file_is_read_only_and_never_written() {
    let (d, mut s) = doc(Some("[[sets]\nid = "));
    assert!(s.read_only());
    assert!(s.error().unwrap().contains("sdc-project.toml"));
    s.set_value("a", "hair", Some(json!(true)));
    assert!(!s.is_dirty());
    assert!(s.save(false).is_err());
    assert_eq!(
        std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap(),
        "[[sets]\nid = "
    );
}

#[test]
fn text_and_restore_round_trip_and_mark_dirty() {
    let (_d, mut s) = doc(Some("[[sets]]\nid = \"a/one\"\n"));
    let before = s.text();
    s.set_value("a/one", "hair", Some(json!(true)));
    let after = s.text();
    s.restore(&before);
    assert_eq!(s.text(), before);
    assert!(s.is_dirty(), "an undo is an unsaved change");
    s.restore(&after);
    assert_eq!(s.entries()[0].settings["hair"], json!(true));
}

#[test]
fn an_outside_change_blocks_a_plain_save() {
    let (d, mut s) = doc(Some("[[sets]]\nid = \"a/one\"\n"));
    s.set_value("a/one", "hair", Some(json!(true)));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(d.path().join("sdc-project.toml"), "# changed elsewhere\n").unwrap();
    assert!(s.changed_on_disk());
    assert_eq!(
        s.save(false),
        Err(sdc_gui::project::SaveError::ChangedOnDisk)
    );
    s.save(true).unwrap();
}

#[test]
fn the_model_opens_saves_and_reports_both_files() {
    use sdc_gui::model::{Dialog, DocKind};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    assert!(!m.is_dirty());
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("landscape/rock01", "hair", Some(json!(true)));
    assert!(m.is_dirty());
    m.save().unwrap();
    assert!(root.join("sdc-project.toml").exists());
    // An outside change to sdc-project.toml names that file.
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("landscape/rock01", "hair", Some(json!(false)));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("sdc-project.toml"), "# elsewhere\n").unwrap();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange(DocKind::Project)));
}

#[test]
fn a_clean_sets_doc_is_never_written_or_checked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    m.project
        .as_mut()
        .unwrap()
        .set_target(engine::config::Target::Cs);
    assert!(m.is_dirty());
    m.save().unwrap();
    assert!(
        !root.join("sdc-project.toml").exists(),
        "a clean sets doc creates no file"
    );
    // A file that appears outside the GUI while the doc is clean is not a conflict.
    std::fs::write(root.join("sdc-project.toml"), "# elsewhere\n").unwrap();
    m.project
        .as_mut()
        .unwrap()
        .set_target(engine::config::Target::Cs);
    m.save().unwrap();
    assert!(m.dialog.is_none(), "{:?}", m.dialog);
}

#[test]
fn a_bad_sdc_project_toml_keeps_sdc_toml_working() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    std::fs::write(root.join("sdc-project.toml"), "[[sets]\nid = ").unwrap();
    m.reload_documents();
    m.wait_idle(std::time::Duration::from_secs(60));
    let banner = m.banner.clone().unwrap();
    assert!(banner.contains("sdc-project.toml"), "{banner}");
    // (c) set edits are a no-op
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("a", "hair", Some(json!(true)));
    assert!(m.sets_doc.as_ref().unwrap().read_only() && !m.sets_doc.as_ref().unwrap().is_dirty());
    // (b) sdc.toml edits still apply and save
    m.project
        .as_mut()
        .unwrap()
        .set_target(engine::config::Target::Cs);
    assert!(m.is_dirty());
    m.save().unwrap();
    assert!(root.join("sdc.toml").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("sdc-project.toml")).unwrap(),
        "[[sets]\nid = "
    );
    // (d) Reload with the file still bad keeps the banner
    m.reload_documents();
    m.wait_idle(std::time::Duration::from_secs(60));
    assert!(m.banner.clone().unwrap().contains("sdc-project.toml"));
    // Fixed: the banner clears.
    std::fs::write(root.join("sdc-project.toml"), "").unwrap();
    m.reload_documents();
    m.wait_idle(std::time::Duration::from_secs(60));
    assert!(m.banner.is_none(), "{:?}", m.banner);
}

#[test]
fn a_non_conflict_save_error_names_the_file() {
    use sdc_gui::model::Dialog;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    // A directory where the temp file goes makes the write fail.
    std::fs::create_dir(root.join("sdc-project.toml.sdc-tmp")).unwrap();
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("a", "hair", Some(json!(true)));
    assert!(m.save().is_err());
    match &m.dialog {
        Some(Dialog::Error(t)) => assert!(t.starts_with("sdc-project.toml: "), "{t}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hand_written_defaults_table_is_ignored_and_left_untouched() {
    let (d, mut s) = doc(Some(
        "[defaults]\nspecular_level = 0.09\nparallax = true\n\n[[sets]]\nid = \"a/one\"\n",
    ));
    assert!(!s.read_only() && s.error().is_none());
    let e = &s.entries()[0];
    assert!(
        e.settings.is_empty(),
        "no default leaks into a set: {:?}",
        e.settings
    );
    s.set_value("a/one", "hair", Some(json!(true)));
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(
        text.contains("[defaults]\nspecular_level = 0.09\nparallax = true\n"),
        "{text}"
    );
}
