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
        .iter()
        .find(|&e| e.id == "a/one")
        .cloned()
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
        .iter()
        .find(|&e| e.id == "landscape/boulder")
        .cloned()
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

/// Windows refuses to replace a file with the read-only flag: the save fails, says how to fix it, leaves the file
/// as it was and leaves no temporary file.
#[cfg(windows)]
#[test]
fn a_non_conflict_save_error_names_the_file() {
    use sdc_gui::model::Dialog;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("a", "hair", Some(json!(true)));
    m.save().unwrap();
    // A read-only file cannot be replaced (Windows), so the write fails.
    let file = root.join("sdc-project.toml");
    let mut perm = std::fs::metadata(&file).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&file, perm.clone()).unwrap();
    let original = std::fs::read(&file).unwrap();
    m.sets_doc
        .as_mut()
        .unwrap()
        .set_value("a", "hair", Some(json!(false)));
    let saved = m.save();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&file, perm).unwrap();
    assert!(saved.is_err());
    assert_eq!(
        m.dialog,
        Some(Dialog::Error(
            "sdc-project.toml is read-only: clear the read-only flag to save".into()
        ))
    );
    assert_eq!(
        std::fs::read(&file).unwrap(),
        original,
        "the file is unchanged"
    );
    let tmp: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".sdc-") && n.ends_with(".tmp"))
        .collect();
    assert!(tmp.is_empty(), "{tmp:?}");
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

#[test]
fn shader_is_reserved_not_a_json_setting() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::sets_doc::SetsDoc::open(d.path());
    s.set_shader("landscape/leaf", Some(Shader::Hair));
    let e = s
        .entries()
        .iter()
        .find(|&e| e.id == "landscape/leaf")
        .cloned()
        .unwrap();
    assert_eq!(e.shader, Some(Shader::Hair));
    assert!(!e.settings.contains_key("shader"), "{:?}", e.settings);
    assert!(s.text().contains("shader = \"hair\""), "{}", s.text());
    s.set_shader("landscape/leaf", None);
    assert!(!s.text().contains("shader"), "{}", s.text());
}

// Final review M1: clearing a shader of a set with no table changes nothing.
#[test]
fn clearing_the_shader_of_a_set_without_a_table_is_a_no_op() {
    let (_d, mut s) = doc(None);
    let before = s.text();
    s.set_shader("landscape/leaf", None);
    assert_eq!(s.text(), before);
    assert!(!s.is_dirty());
}

#[test]
fn set_shader_is_a_no_op_while_read_only() {
    use engine::shader::Shader;
    let (d, mut s) = doc(Some("[[sets]\nid = "));
    assert!(s.read_only());
    let before = s.text();
    s.set_shader("landscape/leaf", Some(Shader::Hair));
    s.set_shader("landscape/leaf", None);
    assert_eq!(s.text(), before);
    assert!(!s.is_dirty());
    assert_eq!(
        std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap(),
        "[[sets]\nid = "
    );
}

#[test]
fn an_unknown_shader_id_reads_as_none_and_survives_a_save() {
    let (d, mut s) = doc(Some(
        "[[sets]]\nid = \"landscape/leaf\"\nshader = \"toon\"\n",
    ));
    assert!(!s.read_only());
    let e = s.entries().iter().next().cloned().unwrap();
    assert_eq!(e.shader, None);
    assert!(!e.settings.contains_key("shader"), "{:?}", e.settings);
    s.set_value("landscape/leaf", "hair", Some(json!(true)));
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(text.contains("shader = \"toon\""), "{text}");
    assert!(text.contains("hair = true"), "{text}");
}

/// AG100: a file moved back into the set that removed it lifts the remove (an `add` beside it would leave it out).
#[test]
fn moving_a_file_back_lifts_its_own_remove() {
    let (_d, mut s) = doc(None);
    s.remove_member("a/wall", "a/wall_n.png");
    assert_eq!(s.entries()[0].remove, vec!["a/wall_n.png".to_string()]);
    s.add_member("a/wall", "a/wall_n.png");
    let e = &s.entries()[0];
    assert!(e.remove.is_empty() && e.add.is_empty(), "{e:?}");
}

/// Trace-debug 2026-10-16 #37: an `add` key without the extension names the file as the engine reads it
/// (`path_key_matches`), so removing that file drops the key instead of refusing or writing a `remove`.
#[test]
fn removing_a_file_moved_in_by_extensionless_key_drops_the_add() {
    let (_d, mut s) = doc(Some(
        "[[sets]]\nid = \"a/new\"\ncreated = true\nadd = [\"a/wall\"]\n",
    ));
    s.remove_member("a/new", "a/wall.png");
    let e = &s.entries()[0];
    assert!(e.add.is_empty() && e.remove.is_empty(), "{e:?}");

    let d = tempfile::tempdir().unwrap();
    let root = support::mod_folder(d.path());
    std::fs::write(
        root.join("sdc-project.toml"),
        "[[sets]]\nid = \"landscape/new\"\ncreated = true\nadd = [\"landscape/odd\"]\n",
    )
    .unwrap();
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings {
            export_folder: Some(d.path().join("export")),
            ..Default::default()
        },
        None,
        support::session(),
    );
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.remove_member("landscape/new", "landscape/odd.png")
        .unwrap();
    let e = m.sets_doc.as_ref().unwrap().entries();
    let e = e.iter().find(|e| e.id == "landscape/new").unwrap();
    assert!(e.add.is_empty() && e.remove.is_empty(), "{e:?}");
}

/// Review M4 (#37, Note BR4): an extension-less `add` key that also names a sibling (`foo` for `foo.png` and
/// `foo.tga`) is not dropped when one of them is removed; the removed file gets its own `remove`, so the sibling
/// stays in the set.
#[test]
fn removing_one_of_two_same_stem_files_keeps_the_shared_add_key() {
    let (d, mut s) = doc(Some(
        "[[sets]]\nid = \"a/new\"\ncreated = true\nadd = [\"a/foo\"]\n",
    ));
    std::fs::create_dir_all(d.path().join("a")).unwrap();
    for f in ["a/foo.png", "a/foo.tga"] {
        std::fs::write(d.path().join(f), b"x").unwrap();
    }
    s.remove_member("a/new", "a/foo.tga");
    let e = &s.entries()[0];
    assert_eq!(e.add, ["a/foo"], "{e:?}");
    assert_eq!(e.remove, ["a/foo.tga"], "{e:?}");
}
