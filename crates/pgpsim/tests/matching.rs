//! 01 §1.1 (preprocessing) and §1.3 (match rules) as table tests.
mod common;
use common::{rock, shape, D, N, NIF};
use pgpsim::{prepare, shape_matches};
use serde_json::{json, Value};

fn case(name: &str, entry: Value, slots: &[&str], want: bool) {
    let m = shape(slots);
    let got = shape_matches(&prepare(&entry, &Value::Null), NIF, &m.slots);
    assert_eq!(got, want, "{name}");
}

#[test]
fn prepare_merges_the_default_shallowly() {
    let p = prepare(
        &json!({"glint": {"screen_space_scale": 2.0}, "parallax": true}),
        &json!({"glint": {"log_microfacet_density": 9.0}, "parallax": false, "emissive": false}),
    );
    assert_eq!(p["glint"], json!({"screen_space_scale": 2.0}));
    assert_eq!(p["parallax"], json!(true));
    assert_eq!(p["emissive"], json!(false));
}

#[test]
fn prepare_copies_texture_over_match_diffuse_and_adds_backslashes() {
    let p = prepare(
        &json!({"texture": "a\\b", "match_diffuse": "c", "match_normal": "n", "rename": "\\r", "path_contains": "pc"}),
        &Value::Null,
    );
    assert_eq!(p["match_diffuse"], json!("\\a\\b"));
    assert_eq!(p["texture"], json!("a\\b"));
    assert_eq!(p["match_normal"], json!("\\n"));
    assert_eq!(p["rename"], json!("\\r"));
    assert_eq!(p["path_contains"], json!("pc"));
}

#[test]
fn prepare_leaves_non_objects_alone() {
    assert_eq!(prepare(&json!(3), &json!({"a": 1})), json!(3));
}

#[test]
fn match_rule_table() {
    case(
        "suffix on a whole component",
        json!({"match_diffuse": "rock01_d"}),
        &[D],
        true,
    );
    case(
        "no partial component",
        json!({"match_diffuse": "rock01_d"}),
        &["textures\\landscape\\bigrock01_d.dds"],
        false,
    );
    case(
        "folders in the value",
        json!({"match_diffuse": "landscape\\rock01_d"}),
        &[D],
        true,
    );
    case(
        "folders must match whole",
        json!({"match_diffuse": "landscape\\rock01_d"}),
        &["textures\\otherlandscape\\rock01_d.dds"],
        false,
    );
    case(
        "diffuse suffix is not stripped",
        json!({"match_diffuse": "rock01"}),
        &[D],
        false,
    );
    case(
        "an extension in the value is dropped",
        json!({"match_diffuse": "rock01_d.dds"}),
        &[D],
        true,
    );
    case(
        "case-insensitive",
        json!({"match_diffuse": "Landscape\\ROCK01_d"}),
        &["Textures\\LandScape\\Rock01_D.DDS"],
        true,
    );
    case(
        "textures\\pbr\\ is stripped on the NIF side",
        json!({"match_diffuse": "landscape\\rock01_d"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        true,
    );
    case(
        "a value starting with textures\\ never matches",
        json!({"match_diffuse": "textures\\landscape\\rock01_d"}),
        &[D],
        false,
    );
    case("texture key", json!({"texture": "rock01_d"}), &[D], true);
    case(
        "texture beats match_diffuse",
        json!({"texture": "rock01_d", "match_diffuse": "other"}),
        &[D],
        true,
    );
    case(
        "match_diffuse is overwritten by texture",
        json!({"texture": "other", "match_diffuse": "rock01_d"}),
        &[D],
        false,
    );
    case(
        "match_normal, _n stripped",
        json!({"match_normal": "rock01"}),
        &[D, N],
        true,
    );
    case(
        "match_normal, _msn stripped",
        json!({"match_normal": "rock01"}),
        &[D, "textures\\landscape\\rock01_msn.dds"],
        true,
    );
    case(
        "match_normal value must not carry _n",
        json!({"match_normal": "rock01_n"}),
        &[D, N],
        false,
    );
    case(
        "match_normal on an already-PBR normal",
        json!({"match_normal": "landscape\\rock01"}),
        &[D, "textures\\pbr\\landscape\\rock01_n.dds"],
        true,
    );
    case(
        "empty diffuse never suffix-matches",
        json!({"match_diffuse": "rock01_d"}),
        &["", N],
        false,
    );
    case(
        "path_contains is a case-insensitive substring",
        json!({"path_contains": "Landscape\\Rock"}),
        &[D],
        true,
    );
    case(
        "path_contains sees no extension",
        json!({"path_contains": ".dds"}),
        &[D],
        false,
    );
    case(
        "path_contains sees no pbr folder",
        json!({"path_contains": "pbr"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        false,
    );
    case(
        "match1 is the full path",
        json!({"match1": "landscape\\rock01_d.dds"}),
        &[D],
        true,
    );
    case(
        "match1 needs .dds",
        json!({"match1": "landscape\\rock01_d"}),
        &[D],
        false,
    );
    case(
        "match2 with its own textures\\ prefix",
        json!({"match2": "textures\\landscape\\rock01_n.dds"}),
        &[D, N],
        true,
    );
    case(
        "the textures\\ prefix check is case-sensitive",
        json!({"match1": "Textures\\landscape\\rock01_d.dds"}),
        &[D],
        false,
    );
    case(
        "match8 is the last one",
        json!({"match8": "x\\y_b.dds"}),
        &[D, N, "", "", "", "", "", "textures\\x\\y_b.dds"],
        true,
    );
    case(
        "there is no match9",
        json!({"match9": "x\\z.dds"}),
        &[D, N, "", "", "", "", "", "", "textures\\x\\z.dds"],
        false,
    );
    case(
        "std::filesystem turns the last separator into \\",
        json!({"match_diffuse": "landscape/rock01_d"}),
        &[D],
        true,
    );
    case(
        "only the last separator is converted",
        json!({"match_diffuse": "a/landscape/rock01_d"}),
        &["textures\\a\\landscape\\rock01_d.dds"],
        false,
    );
    case(
        "no match key at all",
        json!({"roughness_scale": 0.5}),
        &[D, N],
        false,
    );
}

#[test]
fn nif_filter_is_a_case_insensitive_substring_of_the_mesh_path() {
    let m = rock();
    let e = prepare(
        &json!({"match_diffuse": "rock01_d", "nif_filter": "Landscape\\Rocks"}),
        &Value::Null,
    );
    assert!(shape_matches(
        &e,
        "meshes\\landscape\\rocks\\rock01.nif",
        &m.slots
    ));
    assert!(!shape_matches(&e, "meshes\\clutter\\rock01.nif", &m.slots));
}

/// GE2: 2.x applies no `nif_filter` to facegen and BodySlide ShapeData meshes [Src 2.2.0 TP:354-358, 593-596;
/// NU:866-880; measured T5a, T8]. Any case and `/` separators fold first.
#[test]
fn nif_filter_is_ignored_for_facegen_and_bodyslide_shapedata() {
    let m = rock();
    let e = prepare(
        &json!({"match_diffuse": "rock01_d", "nif_filter": "clutter"}),
        &Value::Null,
    );
    for nif in [
        r"Meshes\Actors\Character\FaceGenData\FaceGeom\Skyrim.esm\00000801.nif",
        "meshes/actors/character/facegendata/facegeom/x.esp/00000802.nif",
        r"CalienteTools\BodySlide\ShapeData\Body\body.nif",
    ] {
        assert!(shape_matches(&e, nif, &m.slots), "{nif}");
    }
    assert!(
        !shape_matches(&e, NIF, &m.slots),
        "a normal mesh is filtered"
    );
}

#[test]
fn shape_matches_wants_the_prepared_entry() {
    // Unprepared, "ck01_d" has no leading "\" and hits "rock01_d" mid-component; prepared, it does not.
    let m = rock();
    let raw = json!({"match_diffuse": "ck01_d"});
    assert!(shape_matches(&raw, NIF, &m.slots));
    assert!(!shape_matches(&prepare(&raw, &Value::Null), NIF, &m.slots));
}
