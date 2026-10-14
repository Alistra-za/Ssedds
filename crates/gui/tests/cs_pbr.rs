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
