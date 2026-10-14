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
