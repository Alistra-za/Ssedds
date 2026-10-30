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
    // A long NIF step is shortened on the button; the full text stays in the name and the hover.
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
    assert_eq!(
        b.name,
        "Undo NIF edit: Set the emissive colour of every shape in the mesh"
    );
    assert!(b.hover.contains("every shape in the mesh"));
}
