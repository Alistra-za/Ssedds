//! PGPatcher complex material JSON (complex material design 2026-10-14): the `[sets.cm]` block, the writer, the card
//! predicate, the export and the card's warnings.
mod support;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::collections::BTreeMap;

fn doc(text: &str) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), text).unwrap();
    let s = SetsDoc::open(d.path());
    (d, s)
}

const DOC: &str = "[[sets]]\nid = \"a/one\"\nhair = true\n\n[sets.cm]\n# mine\nexport = true\nglossiness = 80\n\
specular_color = [1, 0.85, 0.6]\nspecular_strength = \"high\"\nmyNote = \"keep\"\n\n[[sets]]\nid = \"b/two\"\nhair = true\n";

#[test]
fn a_cm_block_reads_back_with_unknown_and_wrong_typed_keys_and_never_reaches_the_settings() {
    let (_d, s) = doc(DOC);
    let e = s.entries();
    let b = e[0].cm.as_ref().expect("the block");
    assert!(b.export);
    assert_eq!(b.values["glossiness"], json!(80));
    assert_eq!(b.values["specular_color"], json!([1, 0.85, 0.6]));
    assert_eq!(b.bad, ["specular_strength"]);
    assert!(!b.values.contains_key("myNote"));
    // RESERVED: nothing of the block is a PGPatcher setting.
    assert_eq!(e[0].settings.keys().collect::<Vec<_>>(), ["hair"]);
    assert!(e[1].cm.is_none());
}

/// Review focus 1.
#[test]
fn a_wrong_typed_export_reads_as_off_and_is_listed() {
    let (_d, s) =
        doc("[[sets]]\nid = \"a/one\"\n\n[sets.cm]\nexport = \"yes\"\nspecular_color = [1, 1]\n");
    let b = s.entries()[0].cm.clone().unwrap();
    assert!(!b.export);
    assert_eq!(b.bad, ["export", "specular_color"]);
}

/// Edits keep comments and unknown keys, and a new `[sets.cm]` is printed under its own set.
#[test]
fn cm_edits_keep_the_file_and_land_in_their_own_set() {
    let (d, mut s) = doc(DOC);
    s.set_cm_value("b/two", "dynamic_cubemap", Some(json!(false)));
    s.set_cm_value("a/one", "glossiness", None);
    s.set_cm_export("a/one", false);
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    for keep in [
        "# mine",
        "myNote = \"keep\"",
        "specular_strength = \"high\"",
    ] {
        assert!(text.contains(keep), "{keep} in {text}");
    }
    let s = SetsDoc::open(d.path());
    let e = s.entries();
    let a = e[0].cm.clone().unwrap();
    assert!(!a.export, "unticked");
    assert!(a.values.contains_key("specular_color"), "values are kept");
    assert!(!a.values.contains_key("glossiness"));
    assert_eq!(
        e[1].cm.clone().unwrap().values["dynamic_cubemap"],
        json!(false)
    );
}

#[test]
fn removing_a_key_from_a_set_without_a_block_changes_nothing() {
    let (_d, mut s) = doc("[[sets]]\nid = \"b/two\"\nhair = true\n");
    s.set_cm_value("b/two", "glossiness", None);
    assert!(!s.is_dirty());
    assert!(s.entries()[0].cm.is_none());
}

/// Review focus 4.
#[test]
fn a_read_only_document_takes_no_cm_edit() {
    let (_d, mut s) = doc("[[sets]\nbroken");
    assert!(s.read_only());
    s.set_cm_value("a/one", "glossiness", Some(json!(1.0)));
    s.set_cm_export("a/one", true);
    s.set_cm_json_shas(&BTreeMap::from([("x.json".to_string(), "00".to_string())]));
    assert!(!s.is_dirty());
    assert!(s.cm_json_shas().is_empty());
}

/// Design §3: the hash record reads back, and an undo (restore) keeps the live record.
#[test]
fn the_hash_record_round_trips_and_survives_undo() {
    let (_d, mut s) = doc(DOC);
    let before = s.text();
    s.set_cm_value("a/one", "glossiness", Some(json!(50.0)));
    let shas = BTreeMap::from([("m/iron_m.json".to_string(), "ab12".to_string())]);
    s.set_cm_json_shas(&shas);
    assert_eq!(s.cm_json_shas(), shas);
    assert!(s.text().contains("cm_json_sha256"));
    s.restore_keeping_json_sha(&before);
    assert_eq!(s.cm_json_shas(), shas, "undo keeps the record");
    assert_eq!(
        s.entries()[0].cm.clone().unwrap().values["glossiness"],
        json!(80),
        "undo took the value back"
    );
    s.set_cm_json_shas(&BTreeMap::new());
    assert!(!s.text().contains("cm_json_sha256"));
}

/// Design §3 RESERVED leak: through the model, `cm` keys never reach the TruePBR JSON or `shows_json_panel`.
#[test]
fn model_edits_are_undoable_and_never_reach_the_pgpatcher_json() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::model::sets::{set_views, shows_json_panel};
    use sdc_gui::pgp::writer::{document, to_text};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    for id in ["landscape/rock01", "landscape/leaf"] {
        m.set_cm_export(id, true);
        m.set_cm_value(id, "glossiness", Some(json!(80.0)));
    }
    let entries: Vec<_> = json_entries(&m).into_iter().map(|(_, e)| e).collect();
    let text = to_text(&document(&entries));
    for leak in ["glossiness", "\"cm", "export"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    let leaf = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/leaf")
        .unwrap();
    assert!(leaf.settings.is_empty());
    assert!(!shows_json_panel(&leaf, m.target));
    assert_eq!(
        m.cm_block("landscape/leaf").unwrap().values["glossiness"],
        json!(80.0)
    );
    m.undo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        !m.cm_block("landscape/leaf")
            .unwrap()
            .values
            .contains_key("glossiness"),
        "one step undone"
    );
}
