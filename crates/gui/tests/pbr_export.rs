mod support;
use sdc_gui::model::pbr_export::*;
use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome};
use sdc_gui::model::Dialog;
use serde_json::json;
use std::time::Duration;
use support::*;

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

#[test]
fn a_clean_project_has_no_blocks() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_ready(d.path());
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_feature_without_its_texture_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.set_id == "landscape/rock01" && i.text.contains("emissive")),
        "{b:?}"
    );
    m.export_clicked();
    assert!(m.review.is_some() && m.dialog.is_none(), "{:?}", m.dialog);
    assert!(m.review_gate().unwrap_err().starts_with("Fix "));
}

#[test]
fn exclusive_features_in_one_set_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "hair", Some(json!(true)));
    m.set_json_value("landscape/rock01", "parallax", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("hair") && i.text.contains("parallax")),
        "{b:?}"
    );
}

#[test]
fn a_bad_rename_blocks_and_names_the_field() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "rename", Some(json!(r"..\escape")));
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("rename") && i.text.contains("..")),
        "{b:?}"
    );
}

#[test]
fn a_bad_slot_path_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "slot3", Some(json!(r"..\g.dds")));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("slot3") && i.text.contains("..")),
        "{b:?}"
    );
}

#[test]
fn two_sets_deriving_one_prefix_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    // Point leaf's set at rock01's match: both diffuse outputs land on rock01.dds.
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    m.set_json_value("landscape/leaf", "match1", Some(json!(r"landscape\rock01")));
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("both write")), "{b:?}");
}

#[test]
fn an_owned_output_on_a_mirrored_path_blocks() {
    // The engine skips most such collisions itself (and the plan then fails to settle); this is the check for the
    // ones it does not re-check. Simulate that plan: leaf.png mirrors to the very file rock01 diffuse is sent to.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    assert_eq!(path_collisions(&m), vec![]);
    let leaf = m.items.iter_mut().find(|i| i.name == "leaf.png").unwrap();
    assert!(leaf.job.is_some(), "the fixture leaf has a planned job");
    leaf.job_key = "textures/pbr/landscape/rock01.dds".into();
    m.invalidate_views();
    let b = path_collisions(&m);
    assert!(
        b.iter().any(|i| i.set_id == "landscape/rock01"
            && i.text.contains("textures/pbr/landscape/rock01.dds")
            && i.text.contains("landscape/leaf.png")),
        "{b:?}"
    );
    assert!(export_blocks(&m)
        .iter()
        .any(|i| i.text.contains("leaf.png")));
}

#[test]
fn the_json_of_the_fixture_validates_and_names_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_ready(d.path());
    let entries = json_entries(&m);
    assert!(entries.iter().any(|(v, _)| v.id == "landscape/rock01"));
    let doc =
        sdc_gui::pgp::writer::document(&entries.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>());
    sdc_gui::pgp::validate::validate(&doc.to_value()).unwrap();
    assert_eq!(json_name(&m), "textures", "the opened folder's name");
}

#[test]
fn a_borrowed_slot_is_written_as_the_owners_output_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert!(matches!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    m.reorder_set("landscape/rock01", 0);
    idle(&mut m);
    assert!(set_views(&m).iter().any(|v| v.id == id && v.pbr));
    let (_, e) = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == id)
        .unwrap();
    assert_eq!(
        e.slots.get(&6).map(String::as_str),
        Some(r"textures\pbr\landscape\rock01_rmaos.dds")
    );
}

fn texts(m: &sdc_gui::model::AppModel) -> Vec<String> {
    export_blocks(m).into_iter().map(|i| i.text).collect()
}

#[test]
fn feature_rows_use_the_exact_text() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let id = "landscape/rock01";
    m.set_json_value(id, "emissive", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: emissive is on but the set has no emissive texture (_g)"
        )]
    );
    m.set_json_value(id, "emissive", None);
    m.set_json_value(id, "parallax", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: parallax is on but the set has no height texture (_p)"
        )]
    );
    m.set_json_value(id, "parallax", None);
    m.set_json_value(id, "subsurface", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: subsurface is on but the set has no subsurface texture (_s)"
        )]
    );
    m.set_json_value(id, "subsurface", None);
    m.set_json_value(id, "multilayer", Some(json!(true)));
    m.set_json_value(id, "coat_normal", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: coat normal is on but the set has no coat normal texture (_cnr)"
        )]
    );
}

#[test]
fn fuzz_texture_row_and_a_filled_slot() {
    // The switch is `fuzz_on`; `fuzz.texture` is a sub-setting that lives beside it.
    use sdc_gui::model::sets::SlotKind;
    let mut s = sdc_gui::pgp::settings::Settings::new();
    s.insert("fuzz_on".into(), json!(true));
    s.insert("fuzz.texture".into(), json!(true));
    assert_eq!(
        missing_texture_texts("a/b", &s, |_| false),
        vec!["a/b: fuzz texture is on but the set has no fuzz texture (_f)".to_string()]
    );
    assert!(missing_texture_texts("a/b", &s, |k| k == SlotKind::Fuzz).is_empty());
}

#[test]
fn only_the_matching_slot_silences_a_feature() {
    use sdc_gui::model::sets::SlotKind;
    let one = |key: &str| {
        let mut s = sdc_gui::pgp::settings::Settings::new();
        s.insert(key.into(), json!(true));
        s
    };
    for (key, slot) in [
        ("emissive", SlotKind::Emissive),
        ("parallax", SlotKind::Height),
        ("subsurface", SlotKind::Subsurface),
    ] {
        let s = one(key);
        assert_eq!(missing_texture_texts("x", &s, |_| false).len(), 1, "{key}");
        assert!(
            missing_texture_texts("x", &s, |k| k == slot).is_empty(),
            "{key}"
        );
        let others = |k: SlotKind| k != slot;
        assert_eq!(missing_texture_texts("x", &s, others).len(), 1, "{key}");
    }
}

#[test]
fn coat_and_fuzz_clash_on_any_set() {
    // landscape/zed has an undecided diffuse (so it is not a PBR set) and both a _cnr and an _f.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    for n in ["zed.png", "zed_cnr.png", "zed_f.png"] {
        image::ImageBuffer::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 9u8]))
            .save(r.join(n))
            .unwrap();
    }
    let m = pbr_ready(d.path());
    assert!(set_views(&m)
        .iter()
        .any(|v| v.id == "landscape/zed" && !v.pbr));
    assert_eq!(
        texts(&m),
        vec!["landscape/zed: _cnr and _f both use slot 7".to_string()]
    );
}

#[test]
fn non_pbr_sets_and_vanilla_skip_the_pbr_checks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    // leaf is not a PBR set: a feature with no texture is not a block there.
    m.set_json_value("landscape/leaf", "emissive", Some(json!(true)));
    assert_eq!(export_blocks(&m), vec![]);
    // Vanilla: even a PBR set is not checked, and nothing asks.
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    assert!(!export_blocks(&m).is_empty());
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    assert_eq!(export_asks(&m), vec![]);
    assert!(json_entries(&m).is_empty());
}

#[test]
fn the_json_name_must_be_one_usable_file_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    for bad in [
        r"a\b", "a/b", "..", "x..y", "name.", "name ", "CON", "nul", "Com1", "lpt9.txt", "a:b",
        "a?b",
    ] {
        m.sets_doc.as_mut().unwrap().set_json_name(Some(bad));
        let b = texts(&m);
        assert!(
            b.iter().any(|t| t.starts_with("JSON name:")),
            "{bad:?} should block: {b:?}"
        );
    }
    for good in ["textures", "My Pack", "com10", "console"] {
        m.sets_doc.as_mut().unwrap().set_json_name(Some(good));
        assert_eq!(export_blocks(&m), vec![], "{good:?} should be fine");
    }
}

#[test]
fn the_json_name_is_not_checked_when_there_is_nothing_to_write() {
    let d = tempfile::tempdir().unwrap();
    // No set is decided as PBR, so there is no JSON to write and its name does not matter.
    let mut m = opened(d.path());
    assert!(set_views(&m).iter().all(|v| !v.pbr));
    m.sets_doc.as_mut().unwrap().set_json_name(Some("CON"));
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_borrow_whose_owner_is_blocked_is_reported_once_by_the_owners_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    m.drop_choice(&rmaos, &id, DropChoice::Share);
    m.reorder_set("landscape/rock01", 0);
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![], "the borrow resolves to rock01");
    m.set_json_value("landscape/rock01", "rename", Some(json!(r"..\x")));
    idle(&mut m);
    let b = texts(&m);
    assert!(b.iter().any(|t| t.contains("rename")), "{b:?}");
    assert!(!b.iter().any(|t| t.contains("borrows")), "{b:?}");
}

#[test]
fn glint_and_fuzz_switches_save_with_their_sub_settings_and_reach_the_json() {
    use sdc_gui::pgp::writer::{entry_node, EntryInput};
    use sdc_gui::sets_doc::SetsDoc;
    let d = tempfile::tempdir().unwrap();
    let mut doc = SetsDoc::open(d.path());
    doc.create_set("a/fz");
    doc.create_set("a/gl");
    doc.set_value("a/fz", "fuzz_on", Some(json!(true)));
    doc.set_value("a/fz", "fuzz.texture", Some(json!(true)));
    doc.set_value("a/gl", "glint_on", Some(json!(true)));
    doc.set_value("a/gl", "glint.screen_space_scale", Some(json!(2.0)));
    doc.save(false).unwrap();
    let doc = SetsDoc::open(d.path());
    let entries = doc.entries();
    let of = |id: &str| {
        entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .settings
            .clone()
    };
    let (fz, gl) = (of("a/fz"), of("a/gl"));
    assert_eq!(fz["fuzz.texture"], json!(true));
    assert_eq!(gl["glint.screen_space_scale"], json!(2.0));
    assert!(sdc_gui::pgp::settings::feature_on(
        &fz,
        sdc_gui::pgp::settings::Feature::Fuzz
    ));
    assert!(sdc_gui::pgp::settings::feature_on(
        &gl,
        sdc_gui::pgp::settings::Feature::Glint
    ));
    let json_of = |s| {
        entry_node(&EntryInput {
            settings: s,
            slots: Default::default(),
            derived_match: "ax".into(),
            section: false,
        })
        .to_value()
    };
    let (f, g) = (json_of(fz), json_of(gl));
    assert_eq!(f["fuzz"]["texture"], json!(true));
    assert_eq!(g["glint"]["screen_space_scale"], json!(2.0));
    // The switches themselves are GUI-only and never reach the JSON.
    for v in [&f, &g] {
        assert!(
            v.get("fuzz_on").is_none() && v.get("glint_on").is_none(),
            "{v}"
        );
    }
}

#[test]
fn fuzz_texture_without_an_f_map_blocks_through_the_normal_path() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let id = "landscape/rock01";
    m.set_json_value(id, "fuzz_on", Some(json!(true)));
    m.set_json_value(id, "fuzz.texture", Some(json!(true)));
    assert_eq!(
        texts(&m),
        vec![format!(
            "{id}: fuzz texture is on but the set has no fuzz texture (_f)"
        )]
    );
}

fn export_and_wait(m: &mut sdc_gui::model::AppModel) {
    export_through(m);
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "export did not finish"
    );
}

#[test]
fn export_writes_a_valid_json_and_records_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.05)));
    export_and_wait(&mut m);
    let p = pgp_json_path(&m);
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{p:?}: {e}; notes {:?}", m.export.notes));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    sdc_gui::pgp::validate::validate(&v).unwrap();
    let e = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["match1"] == json!(r"landscape\rock01.dds"))
        .unwrap();
    assert_eq!(e["specular_level"], json!(0.05));
    assert!(m
        .export_root()
        .join("textures/pbr/landscape/rock01_rmaos.dds")
        .exists());
    let sha = m.sets_doc.as_ref().unwrap().last_json_sha256().unwrap();
    assert_eq!(Some(sha), sdc_gui::model::history::sha256_of(&p));
    assert!(!m.is_dirty(), "the hash record was saved");
}

#[test]
fn a_hand_edited_json_asks_before_overwriting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_and_wait(&mut m);
    let p = pgp_json_path(&m);
    std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.06)));
    export_and_wait(&mut m);
    assert_eq!(m.dialog, Some(Dialog::OverwriteJson(p.clone())));
    assert!(std::fs::read_to_string(&p).unwrap().contains("edited"));
    m.resolve_overwrite_json(true);
    assert!(std::fs::read_to_string(&p).unwrap().contains("0.06"));
    assert_eq!(
        m.sets_doc.as_ref().unwrap().last_json_sha256(),
        sdc_gui::model::history::sha256_of(&p)
    );
}

#[test]
fn keeping_a_hand_edited_json_leaves_it_and_the_record() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_and_wait(&mut m);
    let p = pgp_json_path(&m);
    let before = m.sets_doc.as_ref().unwrap().last_json_sha256();
    std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.06)));
    export_and_wait(&mut m);
    m.resolve_overwrite_json(false);
    assert!(m.dialog.is_none());
    assert!(std::fs::read_to_string(&p).unwrap().contains("edited"));
    assert_eq!(m.sets_doc.as_ref().unwrap().last_json_sha256(), before);
}

#[test]
fn finish_leaves_out_sets_whose_outputs_did_not_export() {
    use sdc_gui::model::export::{Row, RowState};
    use sdc_gui::pgp::writer::EntryInput;
    let dir = tempfile::tempdir().unwrap();
    let p = PendingJson {
        root: Default::default(),
        path: dir.path().join("PBRNIFPatcher/x.json"),
        entries: vec![
            (
                "a/one".into(),
                vec!["a/one.dds".into()],
                EntryInput {
                    derived_match: "a\\one".into(),
                    ..Default::default()
                },
            ),
            (
                "a/two".into(),
                vec!["a/two.dds".into()],
                EntryInput {
                    derived_match: "a\\two".into(),
                    ..Default::default()
                },
            ),
        ],
        copies: vec![],
    };
    let rows = vec![
        Row {
            job_key: "a/one.dds".into(),
            path: String::new(),
            state: RowState::Done { passed: true },
            warnings: vec![],
        },
        Row {
            job_key: "a/two.dds".into(),
            path: String::new(),
            state: RowState::Failed("boom".into()),
            warnings: vec![],
        },
    ];
    let mut notes = vec![];
    let r = finish(&p, &rows, None, &mut notes);
    assert!(matches!(r, Finish::Written(_)), "{r:?}");
    let text = std::fs::read_to_string(&p.path).unwrap();
    assert!(
        text.contains(r#""a\\one.dds""#) && !text.contains(r#""a\\two.dds""#),
        "{text}"
    );
    assert!(
        notes.iter().any(|n| n.contains("not in JSON: a/two")),
        "{notes:?}"
    );
}
/// Audit 2026-10-13b H1 (DECISIONS AF1): delete sections alone never replace the JSON.
#[test]
fn finish_with_every_set_failed_keeps_the_old_json_even_with_a_delete_section() {
    use sdc_gui::model::export::{Row, RowState};
    use sdc_gui::pgp::writer::EntryInput;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("PBRNIFPatcher/x.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old good json").unwrap();
    let last = sdc_gui::model::history::sha256_of(&path);
    let mut s = sdc_gui::pgp::settings::Settings::new();
    s.insert("match2".into(), json!("landscape/rock01_n"));
    let section = EntryInput {
        settings: s,
        section: true,
        ..EntryInput::default()
    };
    let set = EntryInput {
        derived_match: "a\\one".into(),
        ..Default::default()
    };
    let p = PendingJson {
        root: Default::default(),
        path: path.clone(),
        entries: vec![
            ("a/one".into(), vec!["a/one.dds".into()], set),
            ("delete section 1".into(), vec![], section.clone()),
        ],
        copies: vec![],
    };
    let rows = vec![Row {
        job_key: "a/one.dds".into(),
        path: String::new(),
        state: RowState::Failed("boom".into()),
        warnings: vec![],
    }];
    let mut notes = vec![];
    assert_eq!(
        finish(&p, &rows, last.as_deref(), &mut notes),
        Finish::Nothing
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old good json");
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("PGPatcher JSON not written: no PBR set exported")),
        "{notes:?}"
    );
    // Zero sets at all: a sections-only plan writes nothing either.
    let only = PendingJson {
        root: Default::default(),
        path: path.clone(),
        entries: vec![("delete section 1".into(), vec![], section)],
        copies: vec![],
    };
    let mut notes = vec![];
    assert_eq!(
        finish(&only, &rows, last.as_deref(), &mut notes),
        Finish::Nothing
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old good json");
    // Review round 1 M6: nothing failed there, and the note says so.
    assert_eq!(
        notes,
        ["PGPatcher JSON not written: there is no PBR set, and delete sections alone are not written"]
    );
}

#[test]
fn finish_copies_borrowed_slots_and_skips_what_did_not_export() {
    use sdc_gui::model::export::{Row, RowState};
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("owner_n.dds");
    std::fs::write(&src, b"dds").unwrap();
    let gone = dir.path().join("gone.dds");
    let stale = dir.path().join("stale.dds");
    std::fs::write(&stale, b"old").unwrap();
    let p = PendingJson {
        root: Default::default(),
        path: std::path::PathBuf::new(),
        entries: vec![],
        copies: vec![
            (
                src.clone(),
                dir.path().join("borrower_n.dds"),
                "o/n.dds".into(),
            ),
            (gone, dir.path().join("x.dds"), "o/gone.dds".into()),
            (stale, dir.path().join("y.dds"), "o/stale.dds".into()),
        ],
    };
    let row = |k: &str, state| Row {
        job_key: k.into(),
        path: String::new(),
        state,
        warnings: vec![],
    };
    let rows = vec![
        row("o/n.dds", RowState::Done { passed: true }),
        row("o/gone.dds", RowState::UpToDate),
        row("o/stale.dds", RowState::Failed("boom".into())),
    ];
    let mut notes = vec![];
    copy_files(&p, &rows, &mut Default::default(), &mut notes);
    assert_eq!(finish(&p, &rows, None, &mut notes), Finish::Nothing);
    assert_eq!(
        std::fs::read(dir.path().join("borrower_n.dds")).unwrap(),
        b"dds"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("copied ") && n.contains('→')),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("could not copy") && n.contains("not exported")),
        "{notes:?}"
    );
    // A failed owner is not copied even though a stale file exists.
    assert!(!dir.path().join("y.dds").exists());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("stale.dds") && n.contains("failed: boom")),
        "{notes:?}"
    );
}

/// A vanilla project where a new set `leaf` borrows rock01's normal: (model, owner output, the copy's path).
fn vanilla_borrow(
    d: &std::path::Path,
) -> (
    sdc_gui::model::AppModel,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let mut m = pbr_ready(d);
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    use sdc_gui::model::sets::{SlotFill, SlotKind};
    let normal = set_views(&m)
        .iter()
        .find(|v| v.id == "landscape/rock01")
        .and_then(|v| match v.slots.get(&SlotKind::Normal) {
            Some(SlotFill::Own { item }) => Some(item.clone()),
            _ => None,
        })
        .expect("rock01 owns a normal map");
    assert!(matches!(
        m.drop_choice(&normal, &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    // Decided, so the owner exports on vanilla and the copy is made.
    m.set_field(&normal, "subtype", json!("landscape_normal"));
    idle(&mut m);
    let views = set_views(&m);
    let borrower = views.iter().find(|v| v.id == id).unwrap();
    let Some(SlotFill::Own { item: d_item }) = borrower.slots.get(&SlotKind::Diffuse) else {
        panic!("the new set owns its diffuse: {:?}", borrower.slots);
    };
    let Some(SlotFill::Borrowed { item: b_item, .. }) = borrower.slots.get(&SlotKind::Normal)
    else {
        panic!("normal is borrowed: {:?}", borrower.slots);
    };
    let dout = m.item(d_item).unwrap().job.as_ref().unwrap().output.clone();
    let src = m.item(b_item).unwrap().job.as_ref().unwrap().output.clone();
    let stem = dout.file_stem().unwrap().to_string_lossy().into_owned();
    let want = dout.with_file_name(format!("{stem}_n.dds"));
    (m, src, want)
}

#[test]
fn prepare_on_vanilla_plans_the_borrowed_slot_copy_with_the_borrowers_name() {
    let d = tempfile::tempdir().unwrap();
    let (m, src, want) = vanilla_borrow(d.path());
    let p = prepare(&m).expect("a borrowed slot on vanilla plans a copy");
    assert!(p.entries.is_empty() && p.path.as_os_str().is_empty());
    assert!(
        p.copies.iter().any(|(s, t, _)| *s == src && *t == want),
        "want {src:?} -> {want:?}, got {:?}",
        p.copies
    );
}

#[test]
fn an_unfinished_export_writes_no_json_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("PBRNIFPatcher/x.json");
    let p = PendingJson {
        root: Default::default(),
        path: path.clone(),
        entries: vec![("a/one".into(), vec![], Default::default())],
        copies: vec![],
    };
    assert_eq!(
        unfinished_note(&p, false).as_deref(),
        Some("PGPatcher JSON not written: the export was cancelled")
    );
    assert_eq!(
        unfinished_note(&p, true).as_deref(),
        Some("PGPatcher JSON not written: the export failed")
    );
    assert!(!path.exists());
    // Vanilla/CPM exports have no JSON, so no note.
    let v = PendingJson {
        root: Default::default(),
        path: Default::default(),
        entries: vec![],
        copies: vec![],
    };
    assert_eq!(unfinished_note(&v, false), None);
}

#[test]
fn an_identical_file_on_disk_is_not_asked_about() {
    use sdc_gui::model::export::{Row, RowState};
    use sdc_gui::pgp::writer::EntryInput;
    let dir = tempfile::tempdir().unwrap();
    let p = PendingJson {
        root: Default::default(),
        path: dir.path().join("x.json"),
        entries: vec![(
            "a/one".into(),
            vec!["a/one.dds".into()],
            EntryInput {
                derived_match: "a\\one".into(),
                ..Default::default()
            },
        )],
        copies: vec![],
    };
    let rows = vec![Row {
        job_key: "a/one.dds".into(),
        path: String::new(),
        state: RowState::UpToDate,
        warnings: vec![],
    }];
    let mut notes = vec![];
    assert!(matches!(
        finish(&p, &rows, None, &mut notes),
        Finish::Written(_)
    ));
    // Same content, no recorded hash: still no prompt.
    assert!(matches!(
        finish(&p, &rows, None, &mut notes),
        Finish::Written(_)
    ));
    std::fs::write(&p.path, "{ \"edited\": true }\n").unwrap();
    assert!(matches!(
        finish(&p, &rows, None, &mut notes),
        Finish::Overwrite(_)
    ));
}

/// Final review C1: a new set whose id sorts before the source set (no reorder) owns the shared `_rmaos`, so it is
/// written at the borrower's path; the source set's entry must then point there too (spec §3.3).
#[test]
fn a_sharing_set_ordered_first_owns_the_output_and_both_entries_point_at_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let leaf = m
        .items
        .iter()
        .find(|i| i.name == "leaf.png")
        .unwrap()
        .key
        .clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    m.set_field(&leaf, "alpha.policy", json!("remove"));
    m.decide(
        &leaf,
        sdc_gui::model::Answer::Acknowledge(engine::codes::Code::LossAlphaData),
    );
    idle(&mut m);
    let id = m.drop_new_set(&leaf).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert!(matches!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    let order: Vec<String> = set_views(&m).into_iter().map(|v| v.id).collect();
    assert!(
        order.iter().position(|i| *i == id) < order.iter().position(|i| i == "landscape/rock01"),
        "the sharing set sorts first: {order:?}"
    );
    let shared = r"textures\pbr\landscape\leaf_rmaos.dds";
    for (v, e) in json_entries(&m) {
        assert_eq!(
            e.slots.get(&6).map(String::as_str),
            Some(shared),
            "{}: {:?}",
            v.id,
            e.slots
        );
    }
    assert_eq!(export_blocks(&m), vec![]);
    export_and_wait(&mut m);
    let out = m.export_root().join("textures/pbr/landscape");
    assert!(out.join("leaf_rmaos.dds").exists());
    assert!(!out.join("rock01_rmaos.dds").exists());
    let p = m
        .export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", json_name(&m)));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{v}; notes {:?}", m.export.notes);
    for e in entries {
        assert_eq!(e["slot6"], json!(shared), "{e}");
    }
}

/// Final review Minor 5 (parked Task 10/11): an owned output whose path is another texture's mirrored output. The
/// engine never sees the clash (the owned path is held back), so the plan settles and Export names the clash instead
/// of failing with "the sets changed while planning".
#[test]
fn an_owned_path_on_a_mirrored_output_settles_and_blocks_by_name() {
    let d = tempfile::tempdir().unwrap();
    let mirrored = d.path().join("textures/textures/pbr/landscape");
    std::fs::create_dir_all(&mirrored).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 4) as u8, 40u8])
    })
    .save(mirrored.join("rock01.png"))
    .unwrap();
    let mut m = pbr_ready(d.path());
    let other = "textures/pbr/landscape/rock01.png";
    m.set_field(other, "subtype", json!("diffuse"));
    idle(&mut m);
    m.export_clicked();
    assert!(m.review.is_some() && m.dialog.is_none(), "{:?}", m.dialog);
    assert!(m.review_gate().unwrap_err().starts_with("Fix "));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.text.contains("landscape/rock01") && i.text.contains(other)),
        "{b:?}"
    );
}

/// Final re-review (5d regression): a source DDS on a PBR output path is skipped (IN-DDS-SKIPPED) and writes
/// nothing, so it must not hold back the PBR path: no block, and the export goes ahead.
#[test]
fn a_skipped_dds_on_a_pbr_output_path_does_not_hold_it_back() {
    let d = tempfile::tempdir().unwrap();
    // A previous export's file kept in the source tree, at the path rock01's diffuse is written to.
    let ref_dir = tempfile::tempdir().unwrap();
    let mut first = pbr_ready(ref_dir.path());
    export_and_wait(&mut first);
    let dds = first
        .export_root()
        .join("textures/pbr/landscape/rock01.dds");
    assert!(dds.exists());
    let kept = d.path().join("textures/textures/pbr/landscape");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::copy(&dds, kept.join("rock01.dds")).unwrap();
    let mut m = pbr_ready(d.path());
    let it = m
        .items
        .iter()
        .find(|i| i.key == "textures/pbr/landscape/rock01.dds")
        .unwrap_or_else(|| panic!("{:?}", m.items.iter().map(|i| &i.key).collect::<Vec<_>>()));
    assert!(
        it.job.is_none(),
        "the source DDS is skipped: {:?}",
        it.status
    );
    assert_eq!(export_blocks(&m), vec![]);
    export_and_wait(&mut m);
    assert!(m.dialog.is_none(), "{:?}", m.dialog);
    assert!(m
        .export_root()
        .join("textures/pbr/landscape/rock01.dds")
        .exists());
}

#[test]
fn emissive_and_skin_sk_clash_on_any_set() {
    // Both use slot 3 (P6). The set is not PBR (undecided diffuse); the clash blocks every set.
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    for n in ["zed.png", "zed_sk.png", "zed_g.png"] {
        image::ImageBuffer::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 9u8]))
            .save(r.join(n))
            .unwrap();
    }
    let mut m = pbr_ready(d.path());
    m.set_field("landscape/zed_sk.png", "subtype", json!("skin_sk"));
    m.set_field("landscape/zed_g.png", "subtype", json!("glow"));
    idle(&mut m);
    assert!(
        texts(&m).contains(&"landscape/zed: _g and _sk both use slot 3".to_string()),
        "{:?}",
        texts(&m)
    );
}

#[test]
fn a_borrowed_skin_sk_is_written_to_slot3() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Luma([x as u8]))
        .save(r.join("rock01_sk.png"))
        .unwrap();
    let mut m = pbr_ready(d.path());
    m.set_field("landscape/rock01_sk.png", "subtype", json!("skin_sk"));
    idle(&mut m);
    m.set_field("landscape/leaf.png", "subtype", json!("pbr_albedo"));
    idle(&mut m);
    let id = m.drop_new_set("landscape/leaf.png").unwrap();
    idle(&mut m);
    assert!(matches!(
        m.drop_choice("landscape/rock01_sk.png", &id, DropChoice::Share),
        DropOutcome::Joined
    ));
    idle(&mut m);
    let (_, e) = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == id)
        .unwrap();
    let p = e.slots.get(&3).expect("slot3 is written");
    assert!(p.ends_with("_sk.dds"), "{p}");
}

#[test]
fn an_explicit_shader_does_not_change_which_sets_get_json() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    let before: Vec<String> = json_entries(&m).into_iter().map(|(v, _)| v.id).collect();
    m.set_shader("landscape/rock01", Some(Shader::Glow));
    m.set_shader("landscape/leaf", Some(Shader::Pbr));
    idle(&mut m);
    let after: Vec<String> = json_entries(&m).into_iter().map(|(v, _)| v.id).collect();
    assert_eq!(before, after);
    assert!(
        after.contains(&"landscape/rock01".to_string())
            && !after.contains(&"landscape/leaf".to_string())
    );
}

/// F1 spec §3.2 "No match", DECISIONS V4: the export still runs, asks first, and the entry stays out of the JSON
/// (the schema refuses an entry without a matcher).
#[test]
fn a_set_that_matches_nothing_asks_and_is_left_out_of_the_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "match1", Some(json!("")));
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    assert!(
        export_asks(&m).iter().any(
            |i| i.set_id == "landscape/rock01" && i.text.contains("this entry matches nothing")
        ),
        "{:?}",
        export_asks(&m)
    );
    assert!(json_entries(&m)
        .iter()
        .all(|(v, _)| v.id != "landscape/rock01"));
}

/// PBR grass design R3, R4: a pbr set switched to pbr_grass exports only grass keys and loses its emissive block;
/// the project file keeps the rest, so switching back restores them.
#[test]
fn a_pbr_grass_set_drops_the_keys_grass_ignores() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m.set_json_value("landscape/rock01", "uv_scale", Some(json!(2.0)));
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.8)));
    idle(&mut m);
    assert!(export_blocks(&m)
        .iter()
        .any(|i| i.text.contains("emissive is on")));
    m.set_shader("landscape/rock01", Some(Shader::PbrGrass));
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    let rock = |m: &sdc_gui::model::AppModel| {
        json_entries(m)
            .into_iter()
            .find(|(v, _)| v.id == "landscape/rock01")
            .unwrap()
            .1
            .settings
    };
    let s = rock(&m);
    assert!(
        !s.contains_key("emissive") && !s.contains_key("uv_scale"),
        "{s:?}"
    );
    assert_eq!(s.get("roughness_scale"), Some(&json!(0.8)));
    assert!(
        !s.contains_key("subsurface"),
        "no _s, so no subsurface default"
    );
    m.set_shader("landscape/rock01", Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(rock(&m).get("uv_scale"), Some(&json!(2.0)));
}

/// Audit 2026-10-13b G-I1 (DECISIONS AF8): the same-name pitfall reaches the export's questions.
#[test]
fn a_renamed_set_whose_name_the_scan_knows_elsewhere_asks_before_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "rename", Some(json!("rock09")));
    idle(&mut m);
    let asks = |m: &sdc_gui::model::AppModel| {
        export_asks(m)
            .iter()
            .any(|i| i.text.contains("textures/dlc/landscape/rock01"))
    };
    assert!(!asks(&m));
    // Through the setter, as the scan read does: it moves `evidence_rev`, which the cached asks are keyed on.
    m.set_known(sdc_gui::model::known_paths::KnownPaths {
        textures: [("textures/dlc/landscape/rock01.dds".to_string(), 1u16)].into(),
        mesh_dirs: vec![],
    });
    assert!(asks(&m), "{:?}", export_asks(&m));
}

/// DECISIONS AS8 (merge of main's auto flags): a pbr_grass set with a `_g` member (SHADER-EXTRA-MAP) exports no
/// emissive and its form does not show it; its `_s` turns subsurface on through main's auto rule, and the writer
/// writes it. Switched back to pbr, the `_g` turns emissive on again.
#[test]
fn a_pbr_grass_set_with_a_g_member_exports_no_emissive() {
    use egui_kittest::kittest::Queryable;
    use engine::shader::Shader;
    const SET: &str = "landscape/rock01";
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    for n in ["rock01_g.png", "rock01_s.png"] {
        image::ImageBuffer::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 9u8]))
            .save(r.join(n))
            .unwrap();
    }
    let mut m = pbr_ready(d.path());
    let rock = |m: &sdc_gui::model::AppModel| {
        json_entries(m)
            .into_iter()
            .find(|(v, _)| v.id == SET)
            .unwrap()
            .1
    };
    assert_eq!(
        rock(&m).settings.get("emissive"),
        Some(&json!(true)),
        "pbr: the _g turns emissive on"
    );
    m.set_shader(SET, Some(Shader::PbrGrass));
    idle(&mut m);
    let e = rock(&m);
    assert!(!e.settings.contains_key("emissive"), "{:?}", e.settings);
    assert_eq!(
        e.settings.get("subsurface"),
        Some(&json!(true)),
        "{:?}",
        e.settings
    );
    let text = sdc_gui::pgp::writer::to_text(&sdc_gui::pgp::writer::document(&[e]));
    // The fixed `default` block always lists `emissive: false`; the set's entry must not.
    let entry = text.split("\"entries\"").nth(1).unwrap();
    assert!(
        !entry.contains("emissive") && entry.contains("\"subsurface\": true"),
        "{text}"
    );
    assert_eq!(export_blocks(&m), vec![]);
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with(SET))
        .unwrap()
        .key
        .clone();
    m.select_set(&key);
    let mut h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 1100.0))
        .build_ui_state(
            |ui, m: &mut sdc_gui::model::AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, SET),
            m,
        );
    h.run();
    assert!(
        h.query_by_label("Emissive").is_none(),
        "the form hides emissive"
    );
    let mut m = h.into_state();
    m.set_shader(SET, Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(rock(&m).settings.get("emissive"), Some(&json!(true)));
}

/// Audit 2026-10-15 item 16: sdc-project.toml changed on disk during an export: recording the JSON's hash asks
/// Overwrite / Reload instead of a dead-end error.
#[test]
fn a_project_file_changed_during_the_export_asks_about_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.05)));
    export_through(&mut m);
    let pf = m.sets_doc.as_ref().unwrap().path().to_path_buf();
    let mut text = std::fs::read_to_string(&pf).unwrap();
    text.push_str("\n# a hand edit\n");
    std::fs::write(&pf, text).unwrap();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(
        m.dialog,
        Some(Dialog::ExternalChange(sdc_gui::model::DocKind::Project))
    );
}

/// Trace-debug 2026-10-16 #43: renaming the PGPatcher JSON leaves the old file with a note (PGPatcher still reads
/// it), and renaming back finds sdc's own file, so it is not asked about as edited outside.
#[test]
fn renamed_json_notes_old_file_and_rename_back_is_not_hand_edited() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_and_wait(&mut m);
    let a = pgp_json_path(&m);
    assert!(a.is_file());
    m.set_json_name("renamed");
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.06)));
    export_and_wait(&mut m);
    let b = pgp_json_path(&m);
    assert!(b.is_file() && a.is_file(), "the old file is left in place");
    let old_name = a.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.contains(&old_name) && n.contains("left in place")),
        "{:?}",
        m.export.notes
    );
    m.set_json_name("");
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.07)));
    export_and_wait(&mut m);
    assert_eq!(pgp_json_path(&m), a);
    assert_eq!(m.dialog, None);
    assert!(std::fs::read_to_string(&a).unwrap().contains("0.07"));
}

/// Audit 08-1 (Note BV3): a borrowed-slot copy never replaces a file Tessalith did not write.
#[test]
fn a_borrowed_slot_copy_keeps_a_foreign_file() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, _src, want) = vanilla_borrow(d.path());
    std::fs::create_dir_all(want.parent().unwrap()).unwrap();
    std::fs::write(&want, b"hand made").unwrap();
    export_and_wait(&mut m);
    assert_eq!(std::fs::read(&want).unwrap(), b"hand made");
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.contains("kept as edited outside Tessalith")),
        "{:?}",
        m.export.notes
    );
}

/// Note BV3: a copy Tessalith made is recorded, so it follows its owner on the next export.
#[test]
fn a_recorded_copy_follows_its_owner() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, src, want) = vanilla_borrow(d.path());
    export_and_wait(&mut m);
    assert_eq!(
        std::fs::read(&want).unwrap(),
        std::fs::read(&src).unwrap(),
        "{:?}",
        m.export.notes
    );
    let mut shas = m.sets_doc.as_ref().unwrap().copy_shas();
    assert_eq!(shas.len(), 1, "{shas:?}");
    // The owner was re-encoded since: the recorded copy is replaced.
    std::fs::write(&src, b"new owner bytes").unwrap();
    let p = prepare(&m).unwrap();
    use sdc_gui::model::export::{Row, RowState};
    let rows: Vec<Row> = p
        .copies
        .iter()
        .map(|(_, _, k)| Row {
            job_key: k.clone(),
            path: String::new(),
            state: RowState::UpToDate,
            warnings: vec![],
        })
        .collect();
    let mut notes = vec![];
    copy_files(&p, &rows, &mut shas, &mut notes);
    assert_eq!(
        std::fs::read(&want).unwrap(),
        b"new owner bytes",
        "{notes:?}"
    );
}

/// Audit 08f-3: a vanilla export that did not end cleanly says its copies were not made.
#[test]
fn an_unfinished_vanilla_export_says_the_copies_were_not_made() {
    let p = PendingJson {
        root: Default::default(),
        path: std::path::PathBuf::new(),
        entries: vec![],
        copies: vec![("a.dds".into(), "b_n.dds".into(), "a.dds".into())],
    };
    assert_eq!(
        unfinished_note(&p, true).as_deref(),
        Some("borrowed-slot copies not made: the export failed")
    );
}

/// Audit 08f-6: an end-of-export question never replaces a dialog that is already open; the hand-edited JSON is
/// kept and a note says so.
#[test]
fn the_overwrite_question_does_not_replace_an_open_dialog() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_ready(d.path());
    export_and_wait(&mut m);
    let p = pgp_json_path(&m);
    std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.06)));
    export_through(&mut m);
    assert!(m.export.running.is_some(), "{:?}", m.dialog);
    // A question that came up while the export ran (here the banner's Reload over unsaved edits).
    m.dialog = Some(Dialog::ReloadUnsaved);
    idle(&mut m);
    assert_eq!(m.dialog, Some(Dialog::ReloadUnsaved));
    assert!(std::fs::read_to_string(&p).unwrap().contains("edited"));
    assert!(
        m.export
            .notes
            .iter()
            .any(|n| n.contains("edited outside Tessalith")),
        "{:?}",
        m.export.notes
    );
}
