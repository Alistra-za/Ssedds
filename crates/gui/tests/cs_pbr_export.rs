//! The CS runtime JSON (design §5): text, files per EDID, the hand-edit guard, refusals and orphans.
mod support;
use sdc_gui::model::cs_pbr::Kind;
use sdc_gui::model::cs_pbr_export::*;
use serde_json::{json, Map, Value as Json};

fn obj(v: Json) -> Map<String, Json> {
    v.as_object().unwrap().clone()
}

fn lines(l: &[&str]) -> String {
    l.join("\n")
}

/// Faultier's LandscapeMountainSlab.json, written from int literals: CS's own layout, every number a float.
#[test]
fn a_texture_set_is_written_as_cs_saves_it_with_decimal_points() {
    let v = obj(json!({
        "displacementScale": 0.2,
        "glintParameters": {"densityRandomization": 300, "enabled": true, "logMicrofacetDensity": 24,
                            "microfacetRoughness": 0.7, "screenSpaceScale": 2},
        "roughnessScale": 1, "specularLevel": 0.04, "subsurfaceColor": [1, 1, 1], "subsurfaceOpacity": 1,
        "notACsField": 5
    }));
    assert_eq!(
        to_text(Kind::Txst, &v),
        lines(&[
            "{",
            "    \"displacementScale\": 0.2,",
            "    \"glintParameters\": {",
            "        \"densityRandomization\": 300.0,",
            "        \"enabled\": true,",
            "        \"logMicrofacetDensity\": 24.0,",
            "        \"microfacetRoughness\": 0.7,",
            "        \"screenSpaceScale\": 2.0",
            "    },",
            "    \"roughnessScale\": 1.0,",
            "    \"specularLevel\": 0.04,",
            "    \"subsurfaceColor\": [",
            "        1.0,",
            "        1.0,",
            "        1.0",
            "    ],",
            "    \"subsurfaceOpacity\": 1.0",
            "}",
        ])
    );
}

#[test]
fn a_material_object_is_written_as_cs_saves_it() {
    let v = obj(json!({
        "baseColorScale": [0.98, 0.98, 1], "roughness": 0.7, "specularLevel": 0.02,
        "glintParameters": {"densityRandomization": 5, "enabled": true, "logMicrofacetDensity": 13,
                            "microfacetRoughness": 1, "screenSpaceScale": 1, "extra": 1}
    }));
    assert_eq!(
        to_text(Kind::Mato, &v),
        lines(&[
            "{",
            "    \"baseColorScale\": [",
            "        0.98,",
            "        0.98,",
            "        1.0",
            "    ],",
            "    \"glintParameters\": {",
            "        \"densityRandomization\": 5.0,",
            "        \"enabled\": true,",
            "        \"logMicrofacetDensity\": 13.0,",
            "        \"microfacetRoughness\": 1.0,",
            "        \"screenSpaceScale\": 1.0",
            "    },",
            "    \"roughness\": 0.7,",
            "    \"specularLevel\": 0.02",
            "}",
        ])
    );
}

#[test]
fn material_specular_is_always_written_and_absent_fields_are_omitted() {
    assert_eq!(
        to_text(Kind::Mato, &obj(json!({"roughness": 0.9}))),
        lines(&[
            "{",
            "    \"roughness\": 0.9,",
            "    \"specularLevel\": 0.04",
            "}"
        ])
    );
    assert_eq!(
        to_text(Kind::Txst, &obj(json!({"specularLevel": 0.04}))),
        lines(&["{", "    \"specularLevel\": 0.04", "}"])
    );
    assert_eq!(
        rel_path(Kind::Mato, "SnowMat1P"),
        "PBRMaterialObjects/SnowMat1P.json"
    );
}

use sdc_gui::model::history::record_key;
use sdc_gui::model::{AppModel, Dialog};
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01";

fn export_and_wait(m: &mut AppModel) {
    // Note BM: adding a texture-set block re-plans (it is shader pick input); export from the settled plan.
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    export_through(m);
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "export did not finish"
    );
}

fn txst_file(m: &AppModel, edid: &str) -> std::path::PathBuf {
    m.export_root()
        .join("PBRTextureSets")
        .join(format!("{edid}.json"))
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn with_blocks(d: &std::path::Path) -> AppModel {
    let mut m = pbr_opened_plain(d);
    m.add_cs_block(ROCK, Kind::Txst);
    m.set_cs_edids(
        ROCK,
        Kind::Txst,
        &["LandscapeRock01".into(), "LandscapeRock01Landscape".into()],
    );
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.02)));
    m.add_cs_block(ROCK, Kind::Mato);
    m.set_cs_edids(ROCK, Kind::Mato, &["SnowMaterialObject1P".into()]);
    m
}

#[test]
fn export_writes_one_file_per_edid_named_exactly_and_records_hashes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    let root = m.export_root();
    assert_eq!(
        names(&root.join("PBRTextureSets")),
        ["LandscapeRock01.json", "LandscapeRock01Landscape.json"],
        "notes {:?}",
        m.export.notes
    );
    let b = m.cs_block(ROCK, Kind::Txst).unwrap();
    assert_eq!(
        std::fs::read_to_string(txst_file(&m, "LandscapeRock01")).unwrap(),
        to_text(Kind::Txst, &b.values)
    );
    let mato =
        std::fs::read_to_string(root.join("PBRMaterialObjects/SnowMaterialObject1P.json")).unwrap();
    assert!(mato.contains("\"specularLevel\": 0.04"), "{mato}");
    let shas = m.sets_doc.as_ref().unwrap().cs_pbr_shas();
    assert_eq!(shas.len(), 3);
    assert_eq!(
        shas.get(&record_key(&txst_file(&m, "LandscapeRock01")))
            .cloned(),
        sdc_gui::model::history::sha256_of(&txst_file(&m, "LandscapeRock01"))
    );
    assert!(!m.is_dirty(), "the record was saved");
    // Undo restores the document but keeps the record of what is on disk.
    m.undo();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.sets_doc.as_ref().unwrap().cs_pbr_shas(), shas);
}

/// Review focus: a file sdc did not write last (edited, or never recorded) is never overwritten.
#[test]
fn a_hand_edited_or_foreign_file_is_kept_and_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    let foreign = txst_file(&m, "LandscapeRock01Landscape");
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, "{\"specularLevel\": 0.5}").unwrap();
    export_and_wait(&mut m);
    assert_eq!(
        std::fs::read_to_string(&foreign).unwrap(),
        "{\"specularLevel\": 0.5}"
    );
    let mine = txst_file(&m, "LandscapeRock01");
    std::fs::write(&mine, "{}").unwrap();
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "{}");
    let kept: Vec<_> = m
        .export
        .notes
        .iter()
        .filter(|n| n.contains("kept as edited outside Tessalith"))
        .collect();
    assert_eq!(kept.len(), 2, "{:?}", m.export.notes);
}

#[test]
fn an_unbound_file_is_reported_not_deleted() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    export_and_wait(&mut m);
    let gone = txst_file(&m, "LandscapeRock01Landscape");
    assert!(gone.is_file(), "sdc never deletes it");
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.contains("LandscapeRock01Landscape.json: no set binds this EDID any more")),
        "{:?}",
        m.export.notes
    );
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .cs_pbr_shas()
        .contains_key(&record_key(&gone)));
}

/// Review focus (O6): CS keys on the exact stem, so a corrected case renames sdc's own file.
#[test]
fn an_edid_whose_case_changed_renames_sdcs_own_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    export_and_wait(&mut m);
    m.set_cs_edids(ROCK, Kind::Txst, &["landscapeRock01".into()]);
    export_and_wait(&mut m);
    assert_eq!(
        names(&m.export_root().join("PBRTextureSets")),
        ["landscapeRock01.json"]
    );
    assert!(
        !m.export.notes.iter().any(|n| n.contains("no set binds")),
        "{:?}",
        m.export.notes
    );
    let shas = m.sets_doc.as_ref().unwrap().cs_pbr_shas();
    let txst: Vec<&String> = shas
        .keys()
        .filter(|k| k.contains("/pbrtexturesets/"))
        .collect();
    assert_eq!(txst, [&record_key(&txst_file(&m, "landscapeRock01"))]);
}

/// Review focus: refusals are per file; EDIDs differing only in case collide on Windows.
#[test]
fn duplicate_and_invalid_edids_are_refused_per_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("sdc-project.toml"),
        "[[sets]]\nid = \"a/one\"\n[sets.cs_txst]\nedids = [\"Dirt\", \"Bad:Name\", \"CON\", \"Fine\"]\n\n\
         [[sets]]\nid = \"b/two\"\n[sets.cs_txst]\nedids = [\"dirt\"]\n[sets.cs_mato]\nedids = [\"Dirt\"]\n",
    )
    .unwrap();
    let doc = sdc_gui::sets_doc::SetsDoc::open(d.path());
    let r = refusals(&doc.entries());
    let why = |id: &str, k: Kind, e: &str| r.get(&(id.to_string(), k, e.to_string())).cloned();
    assert_eq!(
        why("a/one", Kind::Txst, "Dirt").as_deref(),
        Some("PBR texture set Dirt is bound by a/one and b/two")
    );
    assert!(why("b/two", Kind::Txst, "dirt").is_some());
    assert!(why("a/one", Kind::Txst, "Bad:Name")
        .unwrap()
        .contains("Windows does not allow"));
    assert!(why("a/one", Kind::Txst, "CON")
        .unwrap()
        .contains("reserves for a device"));
    assert_eq!(why("a/one", Kind::Txst, "Fine"), None);
    assert_eq!(
        why("b/two", Kind::Mato, "Dirt"),
        None,
        "kinds do not collide"
    );
    assert_eq!(edid_problem(""), Some("EDID: empty (\"\")".into()));
}

/// Task 4 review: `to_text` checks no shapes, so a wrong-typed known key (a 2-element colour, a glint table with
/// no key CS reads, which would be written as `{}`) is left out of the file with a note, never written malformed.
#[test]
fn wrong_typed_values_are_left_out_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    m.set_cs_value(ROCK, Kind::Txst, "subsurfaceColor", Some(json!([1.0, 1.0])));
    m.set_cs_value(ROCK, Kind::Txst, "glintParameters.extra", Some(json!(1)));
    export_and_wait(&mut m);
    let text = std::fs::read_to_string(txst_file(&m, "LandscapeRock01")).unwrap();
    assert_eq!(text, lines(&["{", "    \"specularLevel\": 0.02", "}"]));
    for k in ["subsurfaceColor", "glintParameters"] {
        let note = format!("{ROCK}: cs_txst.{k} has the wrong type and was left out");
        assert!(
            m.export.notes.contains(&note),
            "{note} in {:?}",
            m.export.notes
        );
    }
}

/// Final review (T5 deferred): the case-change rename never deletes an old-case file edited outside sdc.
#[test]
fn a_hand_edited_old_case_file_survives_the_rename() {
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("PBRTextureSets/landscapeRock01.json");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, "{\"roughnessScale\": 1.0}").unwrap();
    let sdc_wrote = sdc_gui::model::history::sha256_of(&old).unwrap();
    std::fs::write(&old, "{\"roughnessScale\": 0.5}").unwrap(); // edited since
    let shas = [("PBRTextureSets/landscapeRock01.json".to_string(), sdc_wrote)].into();
    let p = PendingCs {
        root: d.path().to_path_buf(),
        files: vec![("PBRTextureSets/LandscapeRock01.json".into(), "{}".into())],
        notes: vec![],
        refused: vec![],
    };
    let mut notes = vec![];
    finish(&p, &shas, &mut notes);
    assert!(
        names(&d.path().join("PBRTextureSets")).contains(&"landscapeRock01.json".to_string()),
        "the old-case file is kept"
    );
    assert_eq!(
        std::fs::read_to_string(&old).unwrap(),
        "{\"roughnessScale\": 0.5}"
    );
    // On Windows the new name is the same file, so it is the one reported as kept.
    #[cfg(windows)]
    assert!(
        notes
            .iter()
            .any(|n| n.contains("kept as edited outside Tessalith")),
        "{notes:?}"
    );
}

/// Final review I3: a failed project save after the export never replaces an open question (the overwrite question
/// it was written for is gone, Note FC5; any open dialog stands in).
#[test]
fn a_save_error_leaves_an_open_question_and_goes_to_the_notes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.03)));
    // Saved by the user, so the hash record saves (an unsaved edit waits for Save, Note BD2) and meets the change.
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let toml = m.sets_doc.as_ref().unwrap().path().to_path_buf();
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, format!("{text}\n# edited elsewhere\n")).unwrap();
    let question = Dialog::ExternalChange(sdc_gui::model::DocKind::SdcToml);
    m.dialog = Some(question.clone());
    let p = prepare(&m);
    m.finish_cs_pbr(p);
    assert_eq!(m.dialog, Some(question));
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.starts_with("sdc-project.toml:")),
        "{:?}",
        m.export.notes
    );
    // With no question open the file changed on disk is the save question (audit 2026-10-15 item 16).
    m.dialog = None;
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.01)));
    m.sets_doc.as_mut().unwrap().save(true).unwrap();
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, format!("{text}\n# edited elsewhere again\n")).unwrap();
    let p = prepare(&m);
    m.finish_cs_pbr(p);
    assert_eq!(
        m.dialog,
        Some(Dialog::ExternalChange(sdc_gui::model::DocKind::Project))
    );
}

/// Final review M4: an unreadable scan database (an old schema before a rescan) is said once in the report.
#[test]
fn an_unreadable_scan_database_is_one_report_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    let db = d.path().join("scan.db");
    std::fs::write(&db, "not a database").unwrap();
    m.scan_db = Some(db);
    export_and_wait(&mut m);
    let n: Vec<_> = m
        .export
        .notes
        .iter()
        .filter(|n| n.contains("Rescan"))
        .collect();
    assert_eq!(n.len(), 1, "{:?}", m.export.notes);
    assert!(
        n[0].starts_with("PGPatcher JSON: 1 texture-set set left out"),
        "{}",
        n[0]
    );
}

/// Trace-debug 2026-10-16 #49: an EDID refused because two sets bind it is not also reported as "no set binds this
/// EDID", and its file keeps its record.
#[test]
fn a_refused_recorded_file_is_not_reported_as_unbound() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    m.add_cs_block("landscape/leaf", Kind::Mato);
    m.set_cs_edids(
        "landscape/leaf",
        Kind::Mato,
        &["SnowMaterialObject1P".into()],
    );
    export_and_wait(&mut m);
    let notes = &m.export.notes;
    assert!(
        notes
            .iter()
            .any(|n| n.contains("SnowMaterialObject1P not written")),
        "{notes:?}"
    );
    assert!(
        !notes.iter().any(|n| n.contains("no set binds")),
        "{notes:?}"
    );
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .cs_pbr_shas()
        .contains_key(&record_key(
            &m.export_root()
                .join("PBRMaterialObjects/SnowMaterialObject1P.json")
        )));
}

/// Audit 08c-1: a hash record names its file (absolute path), so exporting to B and back to A still finds A's own
/// files: A's JSON is rewritten, not kept as edited outside Tessalith.
#[test]
fn switching_export_folders_keeps_each_folders_records() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    let a = m.export_root();
    export_and_wait(&mut m);
    m.set_export_folder(d.path().join("b"));
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.03)));
    export_and_wait(&mut m);
    m.set_export_folder(a);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.05)));
    export_and_wait(&mut m);
    let text = std::fs::read_to_string(txst_file(&m, "LandscapeRock01")).unwrap();
    assert!(text.contains("0.05"), "{text}; notes {:?}", m.export.notes);
    assert!(
        !m.export.notes.iter().any(|n| n.contains("kept as edited")),
        "{:?}",
        m.export.notes
    );
}

/// Audit 08c-1: a record from before absolute keys (relative to the export folder) is read as under the current
/// export folder.
#[test]
fn a_relative_record_is_read_under_the_export_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = with_blocks(d.path());
    export_and_wait(&mut m);
    let f = txst_file(&m, "LandscapeRock01");
    let sha = sdc_gui::model::history::sha256_of(&f).unwrap();
    let old = std::collections::BTreeMap::from([(
        "PBRTextureSets/LandscapeRock01.json".to_string(),
        sha,
    )]);
    m.sets_doc.as_mut().unwrap().set_cs_pbr_shas(&old);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.05)));
    export_and_wait(&mut m);
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains("0.05"), "{text}; notes {:?}", m.export.notes);
}

/// Trace-debug wave 2 #48: with no record, an old-case twin this process cannot read is not sdc's file: the
/// case-change rename keeps it (an unread hash is no match for an absent record).
#[cfg(windows)]
#[test]
fn an_unrecorded_unreadable_old_case_file_survives_the_rename() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("PBRTextureSets/foo.json");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, "{\"hand\": 1}").unwrap();
    // FILE_SHARE_DELETE only: other opens for reading fail while this handle is held, a delete does not.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(4)
        .open(&old)
        .unwrap();
    assert!(sdc_gui::model::history::sha256_of(&old).is_none());
    let p = PendingCs {
        root: d.path().to_path_buf(),
        files: vec![("PBRTextureSets/Foo.json".into(), "{}".into())],
        notes: vec![],
        refused: vec![],
    };
    let mut notes = vec![];
    finish(&p, &Default::default(), &mut notes);
    drop(held);
    assert_eq!(
        std::fs::read_to_string(&old).ok().as_deref(),
        Some("{\"hand\": 1}"),
        "{notes:?}"
    );
}
