//! Note FC12: Cancel on an open keeps the previous project, its selection and its undo history (supersedes AG92/AG93's
//! reset); a config error keeps the undo history with the failed edit on top (supersedes BN1's clear); the header's
//! Undo / Redo name the history they act on (CI23, now labelled).
mod support;
use engine::config::Target;
use sdc_gui::model::{Dialog, Selection};
use sdc_gui::ui::header::undo_button;
use sdc_gui::ui::meshes::UndoTarget;
use std::time::Duration;
use support::{mod_folder, opened};

#[test]
fn cancel_keeps_the_previous_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let first = m.root.clone().unwrap();
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.save().unwrap();
    let key = m.items[0].key.clone();
    m.select_item(&key);
    let (steps, label) = (m.history.undo.len(), m.undo_label().map(String::from));
    assert!(steps > 0 && label.is_some());

    let od = tempfile::tempdir().unwrap();
    m.open_folder(mod_folder(od.path()));
    assert_eq!(
        m.cancel_open_hover(),
        "Stops opening and goes back to the previous folder, with its selection and undo history"
    );
    m.cancel_open();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.root.as_ref(), Some(&first));
    assert_eq!(m.selection, Selection::Item(key));
    assert_eq!(m.history.undo.len(), steps, "the undo history is kept");
    assert_eq!(m.undo_label().map(String::from), label);
    assert_eq!(m.target, Target::Vanilla);
    // The kept step still undoes.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Cs);
}

/// Note FC14 (final review, minor): the folder Cancel goes back to finds a crash `.recovery` file, so the re-open pushes
/// its `Restore crash edits` step. That history is kept (it matches the documents), so the toast's Undo takes back the
/// restore, never the older `Mode` step it would otherwise have matched.
#[test]
fn a_crash_restore_on_the_cancel_reopen_keeps_its_history() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.save().unwrap();
    let toml = m.root.clone().unwrap().join("sdc.toml");
    let saved = std::fs::read_to_string(&toml).unwrap();
    let od = tempfile::tempdir().unwrap();
    m.open_folder(mod_folder(od.path()));
    let rec = sdc_gui::crash_log::recovery_path(&toml);
    std::fs::write(&rec, format!("{saved}\n# kept by the crash\n")).unwrap();
    m.cancel_open();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.undo_label(), Some("Restore crash edits"));
    m.undo_crash_restore();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Vanilla, "the Mode step is not undone");
    assert!(!rec.exists(), "the toast's Undo drops the recovery file");
    assert_eq!(m.project.as_ref().unwrap().text(), saved);
}

/// Fix round 1 (6a): "Don't save" drops the edits and their steps before the open starts, so Cancel brings back no
/// step whose state is gone.
#[test]
fn cancel_after_dont_save_brings_back_no_dropped_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let first = m.root.clone().unwrap();
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.can_undo());
    let od = tempfile::tempdir().unwrap();
    m.resolve_open_unsaved(mod_folder(od.path()), false);
    m.cancel_open();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.root.as_ref(), Some(&first));
    assert!(
        !m.can_undo() && !m.can_redo(),
        "the dropped edit's step is gone"
    );
    assert_eq!(m.target, Target::Cs, "the edit was dropped");
}

#[test]
fn a_completed_open_replaces_the_history() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.save().unwrap();
    let od = tempfile::tempdir().unwrap();
    m.open_folder(mod_folder(od.path()));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!m.can_undo(), "another folder starts with its own history");
    assert_eq!(m.selection, Selection::None);
}

#[test]
fn a_config_error_keeps_undo() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let good = m.project.as_ref().unwrap().text();
    let key = m.items[0].key.clone();
    // An out-of-range value parses, so only the engine rejects it when it plans.
    m.edit_docs("Set alpha threshold", None, true, |p, _| {
        p.set_file_value(&key, "alpha.threshold", &serde_json::json!(1000))
    });
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(
        m.project.as_ref().unwrap().read_only(),
        "sdc.toml is blamed"
    );
    assert_eq!(m.dialog, None);
    assert!(m.can_undo(), "the history is kept");
    let top = m.undo_label().unwrap().to_string();
    assert!(
        top.starts_with("Set alpha threshold (sdc.toml rejected it: ") && top.contains("1000"),
        "{top}"
    );
    // Undo takes the failed edit back: the document is usable again, as it was before the edit.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let p = m.project.as_ref().unwrap();
    assert!(!p.read_only());
    assert_eq!(p.text(), good);
    assert_eq!(m.banner, None);
    assert!(!matches!(m.dialog, Some(Dialog::Error(_))));
    assert!(m.can_undo(), "the earlier step is still there");
    assert_eq!(m.undo_label(), Some("Mode"));
    // Note FC14: Redo re-applies the edit, without the old blame in its name.
    assert_eq!(m.redo_label(), Some("Set alpha threshold"));
}

/// Note FC14 (final review, minor): a plan no edit started (here a plain re-plan over a value put into `sdc.toml`
/// without a step) blames no step: the top step keeps its name.
#[test]
fn a_config_error_no_edit_started_blames_no_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.undo_label(), Some("Mode"));
    let key = m.items[0].key.clone();
    m.project
        .as_mut()
        .unwrap()
        .set_file_value(&key, "alpha.threshold", &serde_json::json!(1000));
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(
        m.project.as_ref().unwrap().read_only(),
        "sdc.toml is blamed"
    );
    assert_eq!(m.undo_label(), Some("Mode"), "the step is not blamed");
}

#[test]
fn undo_label_names_the_nif_history() {
    let nif = UndoTarget {
        can_undo: true,
        can_redo: false,
        undo_label: Some("Set shader type".into()),
        redo_label: None,
    };
    let b = undo_button(Some(&nif), true, Some("Mode"));
    assert_eq!(b.text, "Undo NIF edit: Set shader type");
    // Fix round 1 (rule 8.4): the NIF panel's buttons are `Undo NIF edit`; the header's name differs.
    assert_eq!(b.name, "Undo (NIF edit history)");
    assert!(b.enabled);
    assert!(b.hover.contains("NIF edit history"), "{}", b.hover);
    let r = undo_button(Some(&nif), false, Some("Mode"));
    assert_eq!(r.text, "Redo NIF edit");
    assert!(!r.enabled);
    // The Textures history: the plain label, the step and the history on hover.
    let b = undo_button(None, true, Some("Mode"));
    assert_eq!(b.text, "Undo");
    assert!(b.enabled);
    assert!(b.hover.starts_with("Undo: Mode"), "{}", b.hover);
    assert!(b.hover.contains("Textures history"), "{}", b.hover);
    let r = undo_button(None, false, None);
    assert_eq!((r.text.as_str(), r.enabled), ("Redo", false));
    assert_eq!(r.hover, "Nothing to redo");
    // A long NIF step is shortened on the button; the full text stays in the hover.
    let long = UndoTarget {
        undo_label: Some("Set the emissive colour of every shape in the mesh".into()),
        ..nif
    };
    let b = undo_button(Some(&long), true, None);
    assert!(
        b.text.ends_with('…') && b.text.chars().count() <= 40,
        "{}",
        b.text
    );
    assert!(b
        .hover
        .starts_with("Undo NIF edit: Set the emissive colour of every shape in the mesh"));
}
