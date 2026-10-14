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
    let mut m = support::pbr_opened(d.path());
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
