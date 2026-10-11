mod support;
use sdc_gui::model::pbr_export::*;
use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome};
use sdc_gui::model::Dialog;
use serde_json::json;
use std::time::Duration;
use support::*;

fn ready(d: &std::path::Path) -> sdc_gui::model::AppModel {
    let mut m = pbr_opened(d);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

#[test]
fn a_clean_project_has_no_blocks() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_feature_without_its_texture_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(
        b.iter()
            .any(|i| i.set_id == "landscape/rock01" && i.text.contains("emissive")),
        "{b:?}"
    );
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::ExportBlocked(_))));
}

#[test]
fn exclusive_features_in_one_set_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    m.set_json_value(
        "landscape/leaf",
        "match_diffuse",
        Some(json!(r"landscape\rock01")),
    );
    idle(&mut m);
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("both write")), "{b:?}");
}

#[test]
fn an_owned_output_on_a_mirrored_path_blocks() {
    // The engine skips most such collisions itself (and the plan then fails to settle); this is the check for the
    // ones it does not re-check. Simulate that plan: leaf.png mirrors to the very file rock01 diffuse is sent to.
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    assert_eq!(path_collisions(&m), vec![]);
    let leaf = m.items.iter_mut().find(|i| i.name == "leaf.png").unwrap();
    assert!(leaf.job.is_some(), "the fixture leaf has a planned job");
    leaf.job_key = "textures/pbr/landscape/rock01.dds".into();
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
fn pitfalls_ask_and_proceeding_continues_to_the_undecided_check() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    idle(&mut m);
    assert!(!export_asks(&m).is_empty());
    m.export_clicked();
    assert!(
        matches!(m.dialog, Some(Dialog::ExportWarnings(_))),
        "{:?}",
        m.dialog
    );
    m.proceed_with_warnings();
    // The fixture has undecided textures: the M3a question comes next.
    assert!(
        matches!(m.dialog, Some(Dialog::ProceedAnyway(_))),
        "{:?}",
        m.dialog
    );
}

#[test]
fn the_json_of_the_fixture_validates_and_names_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
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
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    let m = ready(d.path());
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
    let mut m = ready(d.path());
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
fn blocks_never_offer_proceed_anyway() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    // Both a block and a pitfall: the block wins and the dialog is the blocked one.
    m.set_json_value("landscape/rock01", "match8", Some(json!(r"x\leaf_s.dds")));
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    idle(&mut m);
    assert!(!export_asks(&m).is_empty());
    m.export_clicked();
    assert!(
        matches!(m.dialog, Some(Dialog::ExportBlocked(_))),
        "{:?}",
        m.dialog
    );
}

#[test]
fn the_json_name_must_be_one_usable_file_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    let mut m = ready(d.path());
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
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "export did not finish"
    );
}

fn json_path(m: &sdc_gui::model::AppModel) -> std::path::PathBuf {
    m.export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", json_name(m)))
}

#[test]
fn export_writes_a_valid_json_and_records_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "specular_level", Some(json!(0.05)));
    export_and_wait(&mut m);
    let p = json_path(&m);
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{p:?}: {e}; notes {:?}", m.export.notes));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    sdc_gui::pgp::validate::validate(&v).unwrap();
    let e = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["match_diffuse"] == json!(r"landscape\rock01"))
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
    let mut m = ready(d.path());
    export_and_wait(&mut m);
    let p = json_path(&m);
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
    let mut m = ready(d.path());
    export_and_wait(&mut m);
    let p = json_path(&m);
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
        text.contains(r#""a\\one""#) && !text.contains(r#""a\\two""#),
        "{text}"
    );
    assert!(
        notes.iter().any(|n| n.contains("not in JSON: a/two")),
        "{notes:?}"
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

#[test]
fn prepare_on_vanilla_plans_the_borrowed_slot_copy_with_the_borrowers_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
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
    let views = set_views(&m);
    let borrower = views.iter().find(|v| v.id == id).unwrap();
    let Some(SlotFill::Own { item: d_item }) = borrower.slots.get(&SlotKind::Diffuse) else {
        panic!("the new set owns its diffuse: {:?}", borrower.slots);
    };
    let Some(SlotFill::Borrowed { item: b_item, .. }) = borrower.slots.get(&SlotKind::Normal)
    else {
        panic!("normal is borrowed: {:?}", borrower.slots);
    };
    let Some(SlotFill::Borrowed { item: b_item, .. }) = borrower.slots.get(&SlotKind::Normal)
    else {
        panic!("normal is borrowed: {:?}", borrower.slots);
    };
    let dout = m.item(d_item).unwrap().job.as_ref().unwrap().output.clone();
    let src = m.item(b_item).unwrap().job.as_ref().unwrap().output.clone();
    let p = prepare(&m).expect("a borrowed slot on vanilla plans a copy");
    assert!(p.entries.is_empty() && p.path.as_os_str().is_empty());
    let stem = dout.file_stem().unwrap().to_string_lossy().into_owned();
    let want = dout.with_file_name(format!("{stem}_n.dds"));
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
