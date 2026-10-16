//! Community Shaders PBR blocks (design 2026-10-13): the template library, blocks in sdc-project.toml, the
//! PGPatcher mapping, the statics rule and the EDID checks.
mod support;
use sdc_gui::model::cs_pbr::*;
use serde_json::{json, Value as Json};
use std::path::PathBuf;

#[test]
fn the_library_parses_with_31_texture_sets_and_3_material_objects() {
    let t = templates();
    assert_eq!(t.iter().filter(|t| t.kind == Kind::Txst).count(), 31);
    assert_eq!(t.iter().filter(|t| t.kind == Kind::Mato).count(), 3);
    assert_eq!(t.iter().map(|t| t.edids.len()).sum::<usize>(), 73 + 31);
    let names: std::collections::BTreeSet<_> = t.iter().map(|t| (t.kind, &t.name)).collect();
    assert_eq!(names.len(), t.len(), "names are unique per kind");
    // Applying any template writes only known fields with usable values.
    for x in t {
        assert!(x.edids.contains(&x.name), "{}", x.name);
        for (k, v) in &x.values {
            let f = field(x.kind, k).unwrap_or_else(|| panic!("{}: unknown field {k}", x.name));
            assert!(valid_value(f.kind, v), "{}: {k} = {v}", x.name);
        }
    }
    let snow = template(Kind::Txst, "LandscapeSnow01").unwrap();
    assert_eq!(snow.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(snow.values["glintParameters"]["enabled"], json!(true));
}

#[test]
fn floats_turn_every_number_into_a_float() {
    assert_eq!(
        floats(&json!({"a": [1, 0.5], "b": {"c": 16}, "d": true})).to_string(),
        r#"{"a":[1.0,0.5],"b":{"c":16.0},"d":true}"#
    );
}

/// Every resource JSON's values equal its template's values (design §9). Reads the resource folder, read-only.
/// `$env:SDC_PBR_RESOURCE='F:\ai\dds\PBR landscape modders resource'; cargo test -p sdc-gui --release --test cs_pbr -- --ignored`
#[test]
#[ignore]
fn every_resource_file_equals_its_template() {
    let root = PathBuf::from(
        std::env::var("SDC_PBR_RESOURCE").expect("set SDC_PBR_RESOURCE to the resource folder"),
    );
    let mut n = 0;
    for kind in [Kind::Txst, Kind::Mato] {
        for e in std::fs::read_dir(root.join(kind.folder())).unwrap() {
            let p = e.unwrap().path();
            let edid = p.file_stem().unwrap().to_str().unwrap().to_string();
            let v: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let t = templates()
                .iter()
                .find(|t| t.kind == kind && t.edids.contains(&edid))
                .unwrap_or_else(|| panic!("no template lists {edid}"));
            assert_eq!(
                floats(&v),
                floats(&Json::Object(t.values.clone())),
                "{edid}"
            );
            n += 1;
        }
    }
    assert_eq!(n, 104);
}

use sdc_gui::sets_doc::SetsDoc;

fn doc(text: &str) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), text).unwrap();
    let s = SetsDoc::open(d.path());
    (d, s)
}

const TWO_SETS: &str = "# notes\n[[sets]]\nid = \"landscape/snow\"\nparallax = true\nroughness_scale = 0.3\n\n\
[sets.cs_txst] # texture set\nedids = [\"LandscapeSnow01\", \"LandscapeSnow01Landscape\"]\nroughnessScale = 1\n\
specularLevel = 0.04\nmyNote = \"keep\"\nglintParameters = { enabled = true, screenSpaceScale = 1.5 }\n\
subsurfaceColor = [1, 0, 0.2]\n\n[sets.cs_mato]\nedids = [\"SnowMaterialObject1P\"]\nbaseColorScale = [0.98, 0.98, 1.0]\n\
roughness = \"rough\"\n\n[[sets]]\nid = \"b/two\"\nhair = true\n";

#[test]
fn blocks_read_back_with_unknown_and_wrong_typed_keys_and_overlay_the_pgpatcher_keys() {
    let (_d, s) = doc(TWO_SETS);
    let e = &s.entries()[0];
    let t = e.cs_txst.as_ref().unwrap();
    assert_eq!(t.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(t.values["roughnessScale"].as_f64(), Some(1.0));
    assert!(!t.values.contains_key("myNote") && t.bad.is_empty());
    let m = e.cs_mato.as_ref().unwrap();
    assert_eq!(m.bad, ["roughness"]);
    assert!(!m.values.contains_key("roughness"));
    // The block is the single source of the keys it maps; unmapped keys stay the set's own.
    assert_eq!(e.settings["roughness_scale"], json!(1.0));
    assert_eq!(e.settings["specular_level"], json!(0.04));
    assert_eq!(e.settings["glint_on"], json!(true));
    assert_eq!(e.settings["glint.screen_space_scale"], json!(1.5));
    assert_eq!(e.settings["subsurface_color"], json!([255, 0, 51]));
    assert_eq!(e.settings["parallax"], json!(true));
    assert!(e
        .settings
        .keys()
        .all(|k| !k.starts_with("cs_") && !k.contains("edids")));
    assert!(s.entries()[1].cs_txst.is_none());
}

#[test]
fn block_edits_keep_comments_and_unknown_keys() {
    let (d, mut s) = doc(TWO_SETS);
    s.set_cs_value(
        "landscape/snow",
        Kind::Txst,
        "displacementScale",
        Some(json!(0.2)),
    );
    s.set_cs_value(
        "landscape/snow",
        Kind::Txst,
        "glintParameters.enabled",
        Some(json!(false)),
    );
    s.set_cs_value("landscape/snow", Kind::Txst, "specularLevel", None);
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    for keep in [
        "# notes",
        "# texture set",
        "myNote = \"keep\"",
        "roughness = \"rough\"",
    ] {
        assert!(text.contains(keep), "{keep} in {text}");
    }
    let s = SetsDoc::open(d.path());
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.values["displacementScale"], json!(0.2));
    assert_eq!(t.values["glintParameters"]["enabled"], json!(false));
    assert!(!t.values.contains_key("specularLevel"));
}

/// Review focus: a new `[sets.cs_txst]` is printed under its own set, not after the last one.
#[test]
fn a_block_added_to_the_first_set_stays_with_it() {
    let (d, mut s) =
        doc("[[sets]]\nid = \"a/one\"\nhair = true\n\n[[sets]]\nid = \"b/two\"\nhair = true\n");
    s.set_cs_value("a/one", Kind::Txst, "edids", Some(json!(["A"])));
    s.save(false).unwrap();
    let s = SetsDoc::open(d.path());
    let e = s.entries();
    assert_eq!(e[0].cs_txst.as_ref().unwrap().edids, ["A"]);
    assert!(e[1].cs_txst.is_none());
}

/// Review focus: adding a texture-set block never drops the set's own PGPatcher values of the keys it maps.
#[test]
fn adding_a_texture_set_block_moves_the_sets_own_mapped_values_into_it() {
    let (_d, mut s) = doc(
        "[[sets]]\nid = \"a/one\"\nroughness_scale = 0.3\nglint_on = true\nglint.screen_space_scale = 2.0\nhair = true\n",
    );
    s.add_cs_block("a/one", Kind::Txst);
    let e = &s.entries()[0];
    let b = e.cs_txst.as_ref().unwrap();
    assert!(b.edids.is_empty());
    assert_eq!(b.values["roughnessScale"], json!(0.3));
    assert_eq!(
        b.values["glintParameters"],
        json!({"enabled": true, "screenSpaceScale": 2.0})
    );
    assert_eq!(e.settings["roughness_scale"], json!(0.3));
    assert_eq!(e.settings["hair"], json!(true));
    assert!(!s.text().contains("roughness_scale"), "moved, not copied");
    s.add_cs_block("a/one", Kind::Txst);
    assert_eq!(
        s.entries()[0].cs_txst.as_ref().unwrap().values["roughnessScale"],
        json!(0.3),
        "a second add changes nothing"
    );
}

/// Final review I2: removing the texture-set block moves its PGPatcher values back to the set (add, remove = identity).
#[test]
fn removing_a_texture_set_block_moves_its_mapped_values_back() {
    let (_d, mut s) = doc(
        "[[sets]]\nid = \"a/one\"\nroughness_scale = 0.3\nsubsurface_color = [255, 0, 51]\nglint_on = true\nglint.screen_space_scale = 2.0\nhair = true\n",
    );
    let before = s.entries()[0].settings.clone();
    s.add_cs_block("a/one", Kind::Txst);
    assert!(!s.text().contains("roughness_scale"));
    s.remove_cs_block("a/one", Kind::Txst);
    let e = &s.entries()[0];
    assert!(e.cs_txst.is_none());
    assert!(!s.text().contains("cs_txst"), "{}", s.text());
    assert_eq!(floats(&json!(e.settings)), floats(&json!(before)));
    assert_eq!(e.settings["subsurface_color"], json!([255, 0, 51]));
}

#[test]
fn a_pgpatcher_edit_of_a_mapped_key_lands_in_the_block() {
    let (_d, mut s) = doc(TWO_SETS);
    s.set_value("landscape/snow", "roughness_scale", Some(json!(0.5)));
    s.set_value(
        "landscape/snow",
        "subsurface_color",
        Some(json!([255, 0, 51])),
    );
    s.set_value("landscape/snow", "glint_on", Some(json!(false)));
    s.set_value("landscape/snow", "hair", Some(json!(true)));
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.values["roughnessScale"], json!(0.5));
    assert_eq!(t.values["subsurfaceColor"], json!([1.0, 0.0, 0.2]));
    assert_eq!(t.values["glintParameters"]["enabled"], json!(false));
    assert!(
        !s.text().contains("roughness_scale"),
        "the stale flat copy is gone"
    );
    assert_eq!(
        s.entries()[0].settings["hair"],
        json!(true),
        "unmapped keys stay flat"
    );
}

#[test]
fn a_template_replaces_values_keeps_unknown_keys_and_the_users_edids() {
    let (_d, mut s) = doc(TWO_SETS);
    let slab = template(Kind::Txst, "LandscapeMountainSlab").unwrap();
    s.apply_cs_template("landscape/snow", slab);
    let t = s.entries()[0].cs_txst.clone().unwrap();
    assert_eq!(t.edids, ["LandscapeSnow01", "LandscapeSnow01Landscape"]);
    assert_eq!(
        floats(&Json::Object(t.values)),
        floats(&Json::Object(slab.values.clone()))
    );
    assert_eq!(t.template.as_deref(), Some("LandscapeMountainSlab"));
    assert!(s.text().contains("myNote"));
    // A block with no EDIDs takes the template's (ruling R8).
    s.set_cs_value("b/two", Kind::Txst, "edids", Some(json!([])));
    s.apply_cs_template("b/two", slab);
    assert_eq!(s.entries()[1].cs_txst.as_ref().unwrap().edids, slab.edids);
}

#[test]
fn model_edits_are_undoable_steps_and_cs_keys_never_reach_the_pgpatcher_json() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::pgp::writer::{document, to_text};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    let id = "landscape/rock01";
    m.add_cs_block(id, Kind::Txst);
    m.apply_cs_template(id, Kind::Txst, "LandscapeMountainSlab")
        .unwrap();
    let b = m.cs_block(id, Kind::Txst).unwrap();
    assert_eq!(b.template.as_deref(), Some("LandscapeMountainSlab"));
    let entries: Vec<_> = json_entries(&m).into_iter().map(|(_, e)| e).collect();
    let text = to_text(&document(&entries));
    for leak in ["cs_txst", "edids", "glintParameters", "roughnessScale"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    assert!(text.contains("\"density_randomization\": 300.0"), "{text}");
    m.undo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(
        m.cs_block(id, Kind::Txst).unwrap().template,
        None,
        "one step undone"
    );
    assert!(m
        .apply_cs_template(id, Kind::Txst, "NoSuchTemplate")
        .is_err());
}

/// Fix round 1: a block created by a value edit or a template moves the set's own mapped values; none stay flat.
#[test]
fn a_block_made_by_an_edit_or_a_template_moves_the_sets_own_mapped_values() {
    const FLAT: &str =
        "[[sets]]\nid = \"a/one\"\nroughness_scale = 0.3\nglint_on = true\nglint.screen_space_scale = 2.0\nhair = true\n";
    let (_d, mut s) = doc(FLAT);
    s.set_cs_value("a/one", Kind::Txst, "statics", None);
    let e = &s.entries()[0];
    let b = e.cs_txst.as_ref().unwrap();
    assert_eq!(b.values["roughnessScale"], json!(0.3));
    assert_eq!(
        b.values["glintParameters"],
        json!({"enabled": true, "screenSpaceScale": 2.0})
    );
    assert_eq!(e.settings["roughness_scale"], json!(0.3));
    assert!(s.is_dirty(), "a created block marks the document dirty");
    assert!(!s.text().contains("roughness_scale") && !s.text().contains("glint_on"));

    // A template replaces the block's known fields (design §4); nothing of the flat keys is left behind.
    let (_d, mut s) = doc(FLAT);
    let slab = template(Kind::Txst, "LandscapeMountainSlab").unwrap();
    s.apply_cs_template("a/one", slab);
    let e = &s.entries()[0];
    assert_eq!(
        floats(&Json::Object(e.cs_txst.clone().unwrap().values)),
        floats(&Json::Object(slab.values.clone()))
    );
    assert!(!s.text().contains("roughness_scale") && !s.text().contains("glint_on"));
    assert_eq!(e.settings["hair"], json!(true));
}

use sdc_gui::model::plugin_refs::set_key;
use sdc_gui::model::sets::{set_views, SetView};
use sdc_gui::model::{AppModel, Dialog};

fn row(path: &str) -> modscan::store::MatchRow {
    modscan::store::MatchRow {
        copy_id: 1,
        nif_path: path.into(),
        origin: "A".into(),
        shape_block: 3,
        shape_name: None,
        le_mesh: false,
        skinned: false,
        derived: false,
        load_copy_id: 1,
    }
}

#[test]
fn lod_meshes_do_not_count_as_statics() {
    let rows = [
        row("meshes/landscape/rocks/rock01.nif"),
        row("meshes/landscape/rocks/rock01.nif"),
        row("meshes/lod/rocks/rock01_lod_0.nif"),
        row("meshes/dyndolod/x.nif"),
        row("meshes/rocks/rock02_lod.nif"),
    ];
    assert_eq!(non_lod_nifs(&rows), ["meshes/landscape/rocks/rock01.nif"]);
}

const ROCK: &str = "landscape/rock01";

fn rock(m: &AppModel) -> SetView {
    set_views(m).into_iter().find(|v| v.id == ROCK).unwrap()
}

/// The fixture's rock set with a texture-set block (LandscapeMountainSlab values) and `facts` as the scan's answer.
fn landscape(d: &std::path::Path, statics: &[&str]) -> AppModel {
    let mut m = support::pbr_opened_plain(d);
    m.add_cs_block(ROCK, Kind::Txst);
    m.apply_cs_template(ROCK, Kind::Txst, "LandscapeMountainSlab")
        .unwrap();
    m.set_cs_edids(ROCK, Kind::Txst, &["LandscapeRock01".into()]);
    let key = set_key(&rock(&m)).unwrap();
    m.cs_pbr.facts.borrow_mut().insert(
        key,
        Ok(SetFacts {
            txst_edids: vec![],
            statics: statics.iter().map(|s| s.to_string()).collect(),
        }),
    );
    m
}

fn export_and_wait(m: &mut AppModel) {
    // Note BM: adding a texture-set block re-plans (it is shader pick input); export from the settled plan.
    assert!(
        m.wait_idle(std::time::Duration::from_secs(60)),
        "planning did not finish"
    );
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(
        m.wait_idle(std::time::Duration::from_secs(120)),
        "export did not finish"
    );
}

fn pgp_json(m: &AppModel) -> Option<Json> {
    let p = m
        .export_root()
        .join("PBRNIFPatcher")
        .join(format!("{}.json", sdc_gui::model::pbr_export::json_name(m)));
    std::fs::read_to_string(p)
        .ok()
        .map(|t| serde_json::from_str(&t).unwrap())
}

#[test]
fn statics_using_the_diffuse_get_the_entry_with_the_blocks_values() {
    let d = tempfile::tempdir().unwrap();
    let mut m = landscape(d.path(), &["meshes/landscape/rocks/rock01.nif"]);
    m.set_cs_value(ROCK, Kind::Txst, "specularLevel", Some(json!(0.02)));
    m.set_cs_value(
        ROCK,
        Kind::Txst,
        "glintParameters.logMicrofacetDensity",
        Some(json!(16)),
    );
    let v = rock(&m);
    assert_eq!(
        m.statics_status(&v),
        Some(StaticsStatus::Used(vec![
            "meshes/landscape/rocks/rock01.nif".into()
        ]))
    );
    assert_eq!(
        statics_text(&m.statics_status(&v).unwrap()),
        "Used by 1 meshes: PGPatcher entry written"
    );
    export_and_wait(&mut m);
    let pgp =
        pgp_json(&m).unwrap_or_else(|| panic!("no PGPatcher JSON; notes {:?}", m.export.notes));
    let e = &pgp["entries"][0];
    let cs: Json = serde_json::from_str(
        &std::fs::read_to_string(m.export_root().join("PBRTextureSets/LandscapeRock01.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(e["specular_level"], json!(0.02));
    assert_eq!(cs["specularLevel"], json!(0.02));
    let g = &cs["glintParameters"];
    assert_eq!(
        e["glint"],
        json!({"screen_space_scale": g["screenSpaceScale"], "log_microfacet_density": 16.0,
               "microfacet_roughness": g["microfacetRoughness"], "density_randomization": g["densityRandomization"]})
    );
    assert_eq!(g["logMicrofacetDensity"], json!(16.0));
}

#[test]
fn only_lod_users_or_never_write_the_texture_set_only() {
    for never in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let statics: &[&str] = if never {
            &["meshes/landscape/rocks/rock01.nif"]
        } else {
            &[]
        };
        let mut m = landscape(d.path(), statics);
        if never {
            m.set_cs_statics(ROCK, Statics::Never);
        }
        assert!(!m.pgp_entry_wanted(&rock(&m)));
        export_and_wait(&mut m);
        assert!(pgp_json(&m).is_none(), "no entry, so no PGPatcher JSON");
        assert!(m
            .export_root()
            .join("PBRTextureSets/LandscapeRock01.json")
            .is_file());
    }
}

#[test]
fn a_disabled_glint_writes_no_glint() {
    let d = tempfile::tempdir().unwrap();
    let mut m = landscape(d.path(), &["meshes/landscape/rocks/rock01.nif"]);
    m.set_cs_value(
        ROCK,
        Kind::Txst,
        "glintParameters.enabled",
        Some(json!(false)),
    );
    export_and_wait(&mut m);
    assert!(pgp_json(&m).unwrap()["entries"][0].get("glint").is_none());
}

/// Review focus: a scan database that cannot be read is "no scan", never a crash and never an entry.
#[test]
fn an_unusable_scan_db_writes_no_statics_entry_unless_always() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    m.add_cs_block(ROCK, Kind::Txst);
    let bad = d.path().join("scan.db");
    std::fs::write(&bad, "not a database").unwrap();
    m.scan_db = Some(bad);
    let v = rock(&m);
    assert!(matches!(
        m.statics_status(&v),
        Some(StaticsStatus::NoScan(_))
    ));
    assert!(!m.pgp_entry_wanted(&v));
    m.set_cs_statics(ROCK, Statics::Always);
    assert_eq!(m.statics_status(&rock(&m)), Some(StaticsStatus::Always));
    assert!(m.pgp_entry_wanted(&rock(&m)));
    // A set without a texture-set block is unchanged.
    m.remove_cs_block(ROCK, Kind::Txst);
    assert_eq!(m.statics_status(&rock(&m)), None);
    assert!(m.pgp_entry_wanted(&rock(&m)));
}

/// Ruling P2: an answer (an error too) is kept while the database file is unchanged, and re-read once its length
/// or mtime changes.
#[test]
fn scan_reads_are_cached_until_the_database_file_changes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    let db = d.path().join("scan.db");
    std::fs::write(&db, "not a database").unwrap();
    m.scan_db = Some(db.clone());
    m.poll(); // the frame stamp (Note BI4) is what the cache follows (Note BM)
    let key = set_key(&rock(&m)).unwrap();
    assert!(m.scan_facts(&key).is_err());
    assert!(m.scan_records().is_err());
    let sentinel = SetFacts {
        txst_edids: vec!["Cached".into()],
        statics: vec![],
    };
    m.cs_pbr
        .facts
        .borrow_mut()
        .insert(key.clone(), Ok(sentinel.clone()));
    assert_eq!(
        m.scan_facts(&key),
        Ok(sentinel.clone()),
        "unchanged file: not re-opened"
    );
    std::fs::write(&db, "not a database!").unwrap();
    m.poll();
    assert!(m.scan_facts(&key).is_err(), "a new length re-reads");
    m.cs_pbr
        .facts
        .borrow_mut()
        .insert(key.clone(), Ok(sentinel));
    let f = std::fs::File::options().write(true).open(&db).unwrap();
    f.set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
    drop(f);
    m.poll();
    assert!(m.scan_facts(&key).is_err(), "a new mtime re-reads");
}

/// Ruling P5: a set whose entry the statics rule drops writes no JSON, so it cannot block the export.
#[test]
fn a_dropped_entry_does_not_block_the_export() {
    use sdc_gui::model::pbr_export::export_blocks;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    m.sets_doc.as_mut().unwrap().set_json_name(Some("CON"));
    let name_block = |m: &AppModel| {
        export_blocks(m)
            .iter()
            .any(|i| i.text.starts_with("JSON name:"))
    };
    assert!(name_block(&m), "the only entry blocks on its JSON name");
    m.add_cs_block(ROCK, Kind::Txst); // no scan: the entry is dropped
    assert!(!m.pgp_entry_wanted(&rock(&m)));
    assert!(!name_block(&m), "{:?}", export_blocks(&m));
    m.set_cs_statics(ROCK, Statics::Always);
    assert!(name_block(&m));
}

fn slot_block(m: &AppModel) -> bool {
    sdc_gui::model::pbr_export::export_blocks(m)
        .iter()
        .any(|i| i.text.contains("slot3"))
}

/// Fix round 1: none of a dropped set's own checks block the export.
#[test]
fn a_dropped_set_with_a_bad_slot_path_does_not_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    m.set_json_value(ROCK, "slot3", Some(json!(r"..\g.dds")));
    m.add_cs_block(ROCK, Kind::Txst);
    m.set_cs_statics(ROCK, Statics::Never);
    assert!(
        !slot_block(&m),
        "{:?}",
        sdc_gui::model::pbr_export::export_blocks(&m)
    );
    m.set_cs_statics(ROCK, Statics::Always);
    assert!(slot_block(&m));
}

/// Fix round 1: the export blocks follow a new scan (the scan file is part of their cache key).
#[test]
fn export_blocks_follow_a_rescan_both_ways() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(d.path());
    m.set_json_value(ROCK, "slot3", Some(json!(r"..\g.dds")));
    m.add_cs_block(ROCK, Kind::Txst);
    let db = d.path().join("scan.db");
    std::fs::write(&db, "scan 1").unwrap();
    m.scan_db = Some(db.clone());
    let key = set_key(&rock(&m)).unwrap();
    let used = || {
        Ok(SetFacts {
            txst_edids: vec![],
            statics: vec!["meshes/landscape/rocks/rock01.nif".into()],
        })
    };
    // Scan 1: a mesh uses the set, so its bad slot blocks.
    let _ = m.scan_facts(&key);
    m.cs_pbr.facts.borrow_mut().insert(key.clone(), used());
    assert!(slot_block(&m));
    // Scan 2 (a new file length): no usable answer, the entry is dropped and the cached block goes.
    std::fs::write(&db, "scan 2!").unwrap();
    m.poll(); // the next frame stats the database (REPORT #9)
    assert!(!slot_block(&m));
    // Scan 3: used again, the block comes back.
    std::fs::write(&db, "scan 3!!").unwrap();
    m.poll();
    let _ = m.scan_facts(&key);
    m.cs_pbr.facts.borrow_mut().insert(key, used());
    assert!(slot_block(&m));
}

/// Fix round 1: LOD shapes filling the row cap never hide a static after them.
#[test]
fn lod_rows_filling_the_cap_do_not_hide_a_static() {
    let all: Vec<_> = (0..5)
        .map(|i| row(&format!("meshes/lod/r{i}_lod_0.nif")))
        .chain([row("meshes/landscape/rocks/rock01.nif")])
        .collect();
    let read = |n: usize| Ok(all.iter().take(n).cloned().collect());
    assert_eq!(
        statics_of(5, read).unwrap(),
        ["meshes/landscape/rocks/rock01.nif"]
    );
    assert_eq!(
        statics_of(6, read).unwrap(),
        ["meshes/landscape/rocks/rock01.nif"]
    );
    let lod_only = |n: usize| Ok(all[..5].iter().take(n).cloned().collect());
    assert!(statics_of(5, lod_only).unwrap().is_empty());
    assert!(statics_of(9, lod_only).unwrap().is_empty());
}

fn win(edid: &str, flags: Option<u32>) -> modscan::plugin_store::WinRec {
    modscan::plugin_store::WinRec {
        formid: 1,
        edid: edid.into(),
        plugin: "Skyrim.esm".into(),
        mato_flags: flags,
    }
}

#[test]
fn edid_checks_find_case_mismatches_and_single_pass_status() {
    let txst = [win("LandscapeSnow01", None)];
    assert_eq!(
        check_edid(Kind::Txst, "LandscapeSnow01", &txst),
        EdidCheck::Found
    );
    assert_eq!(
        check_edid(Kind::Txst, "landscapesnow01", &txst),
        EdidCheck::Case("LandscapeSnow01".into())
    );
    assert_eq!(check_edid(Kind::Txst, "Nope", &txst), EdidCheck::NotFound);
    // Flags come from DATA offset 44 (48-byte DATA included); None only when DATA is too short.
    let mato = [
        win("Snow1P", Some(1)),
        win("Plain", Some(0)),
        win("Ash1P", None),
    ];
    assert_eq!(check_edid(Kind::Mato, "Snow1P", &mato), EdidCheck::Found);
    assert_eq!(
        check_edid(Kind::Mato, "Plain", &mato),
        EdidCheck::NotSinglePass
    );
    assert_eq!(
        check_edid(Kind::Mato, "Ash1P", &mato),
        EdidCheck::SinglePassUnknown
    );
    assert_eq!(check_text(&EdidCheck::Found), None);
    assert_eq!(
        check_text(&EdidCheck::NotSinglePass),
        Some("CS ignores this JSON unless the record is single-pass")
    );
    assert_eq!(single_pass_text(Some(1)), "single-pass");
}
