mod support;
use sdc_gui::model::history::{Command, History, Snapshot};
use serde_json::json;
use support::*;

fn snap(a: &str, b: &str) -> Snapshot {
    Snapshot {
        sdc_toml: a.into(),
        project: b.into(),
    }
}

fn cmd(label: &str, before: Snapshot, after: Snapshot, merge: Option<&str>) -> Command {
    Command {
        label: label.into(),
        before,
        after,
        merge: merge.map(str::to_string),
        created_file: None,
    }
}

#[test]
fn history_undoes_redoes_merges_and_caps() {
    let mut h = History::default();
    h.push(cmd("a", snap("0", ""), snap("1", ""), None));
    h.push(cmd("slider", snap("1", ""), snap("2", ""), Some("x")));
    h.push(cmd("slider", snap("2", ""), snap("3", ""), Some("x")));
    assert_eq!(h.undo.len(), 2, "two steps with the same merge key are one");
    let c = h.undo_step().unwrap();
    assert_eq!(
        (c.before.sdc_toml.as_str(), c.after.sdc_toml.as_str()),
        ("1", "3")
    );
    assert!(h.can_redo());
    h.push(cmd("b", snap("1", ""), snap("9", ""), None));
    assert!(!h.can_redo(), "a new edit drops the redo list");
    for i in 0..(History::LIMIT + 10) {
        h.push(cmd("n", snap(&i.to_string(), ""), snap("x", ""), None));
    }
    assert_eq!(h.undo.len(), History::LIMIT);
}

#[test]
fn every_kind_of_edit_undoes_and_redoes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let item = m
        .items
        .iter()
        .find(|i| i.name == "rock01.png")
        .unwrap()
        .key
        .clone();
    // A [files] edit (sdc.toml).
    m.set_field(&item, "mips.levels", json!(3));
    assert_eq!(
        m.project.as_ref().unwrap().file_value(&item, "mips.levels"),
        Some(json!(3))
    );
    m.undo();
    assert_eq!(
        m.project.as_ref().unwrap().file_value(&item, "mips.levels"),
        None
    );
    m.redo();
    assert_eq!(
        m.project.as_ref().unwrap().file_value(&item, "mips.levels"),
        Some(json!(3))
    );
    // Mode.
    m.set_target(engine::config::Target::Vanilla);
    assert_eq!(
        m.project.as_ref().unwrap().target(),
        Some(engine::config::Target::Vanilla)
    );
    m.undo();
    assert_eq!(m.project.as_ref().unwrap().target(), None);
    // A sets-doc edit.
    m.edit_docs("hair", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(true)))
    });
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .any(|e| e.settings.contains_key("hair")));
    m.undo();
    assert!(!m
        .sets_doc
        .as_ref()
        .unwrap()
        .entries()
        .iter()
        .any(|e| e.settings.contains_key("hair")));
    assert!(
        m.is_dirty(),
        "undo back past the save point stays an unsaved change"
    );
}

#[test]
fn undo_after_an_outside_edit_never_overwrites_silently() {
    use sdc_gui::model::{Dialog, DocKind};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let root = m.root.clone().unwrap();
    m.edit_docs("hair", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(true)))
    });
    m.save().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("sdc-project.toml"), "# edited elsewhere\n").unwrap();
    m.undo();
    assert!(m.save().is_err());
    assert_eq!(
        m.dialog,
        Some(Dialog::ExternalChange(DocKind::Project)),
        "Review focus 4"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("sdc-project.toml")).unwrap(),
        "# edited elsewhere\n"
    );
}

#[test]
fn opening_another_folder_clears_the_history() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.can_undo());
    m.save().unwrap();
    m.open_folder(mod_folder(&d.path().join("b")));
    assert!(!m.can_undo() && !m.can_redo());
}

#[test]
fn an_edit_that_changes_nothing_is_not_a_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.edit_docs("noop", None, false, |_, _| {});
    assert!(!m.can_undo());
}

#[test]
fn sdc_toml_edits_undo_while_the_sets_doc_is_read_only() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc-project.toml"), "this is = = not toml\n").unwrap();
    let mut m =
        sdc_gui::model::AppModel::new(sdc_gui::settings::GuiSettings::default(), None, session());
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    m.set_target(engine::config::Target::Vanilla);
    assert_eq!(
        m.project.as_ref().unwrap().target(),
        Some(engine::config::Target::Vanilla)
    );
    assert!(m.can_undo());
    m.undo();
    assert_eq!(m.project.as_ref().unwrap().target(), None);
    m.redo();
    assert_eq!(
        m.project.as_ref().unwrap().target(),
        Some(engine::config::Target::Vanilla)
    );
    // The read-only sets doc was never dirtied or made writable by undo/redo.
    let s = m.sets_doc.as_ref().unwrap();
    assert!(s.read_only() && !s.is_dirty());
}

#[test]
fn undo_of_an_sdc_toml_edit_leaves_the_sets_doc_clean_and_save_writes_no_project_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let root = m.root.clone().unwrap();
    m.set_target(engine::config::Target::Vanilla);
    m.undo();
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    m.save().unwrap();
    assert!(!root.join("sdc-project.toml").exists());
    assert_eq!(m.dialog, None);
}

#[test]
fn undo_of_a_sets_only_edit_leaves_sdc_toml_clean() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.edit_docs("hair", None, false, |_, s| {
        s.set_value("landscape/rock01", "hair", Some(json!(true)))
    });
    m.undo();
    assert!(!m.project.as_ref().unwrap().is_dirty());
}

#[test]
fn a_plan_rejection_clears_the_history() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let item = m
        .items
        .iter()
        .find(|i| i.name == "rock01.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&item, "alpha.threshold", json!(1000));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        m.project.as_ref().unwrap().read_only(),
        "the engine rejected it"
    );
    assert!(!m.can_undo() && !m.can_redo());
}
