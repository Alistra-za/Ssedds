//! P1-D: shape-keyed CS settings rules and the coverage report over the four CS builds the owner has
//! (03-cs-settings-drift.md). Fixtures = name lists extracted by the P1-D plan Task 1 (no CS source, no CS values).
use render3d::cs_settings::{
    coverage, feature_data, linear_lighting_on, read_settings, Coverage, CsSettings, SettingsPaths,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

const BUILDS: [&str; 4] = ["cs16_0528", "cs18_bottle", "nexus191", "aio190"];

fn dir(tag: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_versions"
    ))
    .join(tag)
}

fn fields_of(tag: &str) -> BTreeSet<String> {
    std::fs::read_to_string(dir(tag).join("fields.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// The build's reflected names and its SettingsDefault key set, read like a real settings file.
fn build(tag: &str) -> (BTreeSet<String>, CsSettings) {
    let s = read_settings(&SettingsPaths {
        default: Some(dir(tag).join("keys.json")),
        ..Default::default()
    });
    assert!(!s.parse_failed, "{tag}: {:?}", s.notes);
    (fields_of(tag), s)
}

fn value(fd: &[(String, Vec<f64>)], name: &str) -> Vec<f64> {
    fd.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} not written"))
        .1
        .clone()
}

/// The evidence each build carries (03 report §2): which LL generation, b7 profile, b8, Fresnel, skin JSON shape.
#[test]
fn fixtures_carry_each_builds_evidence() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let new = tag == "aio190";
        assert_eq!(
            f.contains("linearLightingSettings.diffuseCurve"),
            new,
            "{tag}"
        );
        assert_eq!(
            f.contains("linearLightingSettings.isDirLightLinear"),
            !new,
            "{tag}"
        );
        assert_eq!(
            f.contains("skinPerGeometryProfile.skinParams"),
            new,
            "{tag}"
        );
        assert_eq!(f.contains("emissiveMult"), !new, "{tag}: b8 LLPerGeometry");
        assert_eq!(
            f.contains("vanillaFresnelSettings.Enable"),
            tag != "nexus191",
            "{tag}"
        );
        let skin = s.merged.get("Advanced Skin");
        match tag {
            "nexus191" => assert!(s.merged.is_empty(), "no JSON until CS first runs"),
            "aio190" => assert!(skin.unwrap().get("DefaultProfile").is_some()),
            _ => assert!(
                skin.unwrap().get("SkinDetailTiling").is_some(),
                "{tag}: flat skin"
            ),
        }
    }
}

/// Task 1 changes no value: with the installed build's names, feature_data equals what P1-C wrote.
#[test]
fn the_installed_build_keeps_the_p1c_values() {
    let fd = feature_data(&CsSettings::default(), &fields_of("aio190"), true, false);
    assert_eq!(
        (value(&fd, "linearLightingSettings.pointLightMult")[0] as f32).to_bits(),
        std::f32::consts::PI.to_bits()
    );
    assert_eq!(
        value(&fd, "skinData.skinDetailParams"),
        [20.0, 2.0, 0.25, 0.0]
    );
}

/// Settings as `read_settings` would give them for a file with exactly `json`.
fn with(json: serde_json::Value) -> CsSettings {
    let merged = json.as_object().unwrap().clone();
    let linear_lighting = merged
        .get("Linear Lighting")
        .and_then(|l| l.get("enableLinearLighting"))
        .and_then(|v| v.as_f64())
        .is_none_or(|x| x != 0.0);
    CsSettings {
        merged,
        linear_lighting,
        ..CsSettings::default()
    }
}

const PI: f64 = std::f32::consts::PI as f64;

/// New LL (diffuseCurve reflected): the CPU uploads π × pointLightMult (dev LinearLighting.cpp:228-229).
#[test]
fn new_linear_lighting_uploads_pi_times_the_setting() {
    let s = with(
        serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "pointLightMult": 2.0}}),
    );
    let fd = feature_data(&s, &fields_of("aio190"), true, false);
    assert_eq!(
        value(&fd, "linearLightingSettings.pointLightMult"),
        [PI * 2.0]
    );
    assert_eq!(
        value(&fd, "linearLightingSettings.directionalLightMult"),
        [1.0],
        "directional stays raw (capture 11178)"
    );
}

/// New LL inactive (setting off, or the features toggle off): every multiplier and fireEffectCurve = 1.0.
#[test]
fn new_linear_lighting_off_writes_every_multiplier_as_one() {
    let f = fields_of("aio190");
    let off = with(
        serde_json::json!({"Linear Lighting": {"enableLinearLighting": 0, "glowmapMult": 0.66}}),
    );
    let on = with(
        serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "glowmapMult": 0.66}}),
    );
    for (s, features_on) in [(&off, true), (&on, false)] {
        let fd = feature_data(s, &f, features_on, false);
        let mults: Vec<&String> = f
            .iter()
            .filter(|n| {
                n.starts_with("linearLightingSettings.")
                    && (n.ends_with("Mult") || n.ends_with("fireEffectCurve"))
            })
            .collect();
        assert_eq!(mults.len(), 14, "13 *Mult + fireEffectCurve: {mults:?}");
        for n in mults {
            assert_eq!(value(&fd, n), [1.0], "{n}");
        }
    }
    let fd = feature_data(&on, &f, true, false);
    assert_eq!(
        bits(&value(&fd, "linearLightingSettings.glowmapMult")),
        [0.66f32.to_bits()]
    );
}

fn bits(v: &[f64]) -> Vec<u32> {
    v.iter().map(|x| (*x as f32).to_bits()).collect()
}

/// Old LL (isDirLightLinear reflected): raw multipliers (the shader applies π), the engine fields and b8
/// emissiveMult set, and the LL flag = the setting; no JSON (Nexus) = off.
#[test]
fn old_linear_lighting_writes_raw_values_and_the_engine_fields() {
    let f = fields_of("cs18_bottle");
    let on = with(
        serde_json::json!({"Linear Lighting": {"enableLinearLighting": 1, "pointLightMult": 1.0}}),
    );
    let fd = feature_data(&on, &f, true, false);
    assert_eq!(value(&fd, "linearLightingSettings.pointLightMult"), [1.0]);
    assert_eq!(
        value(&fd, "linearLightingSettings.enableLinearLighting"),
        [1.0]
    );
    assert_eq!(value(&fd, "linearLightingSettings.isDirLightLinear"), [0.0]);
    assert_eq!(value(&fd, "linearLightingSettings.dirLightMult"), [1.0]);
    assert_eq!(value(&fd, "emissiveMult"), [1.0]);
    let toggled_off = feature_data(&on, &f, false, false);
    assert_eq!(
        value(&toggled_off, "linearLightingSettings.enableLinearLighting"),
        [0.0]
    );
    let none = CsSettings::default();
    assert!(
        !linear_lighting_on(&none, true, true),
        "no JSON: the old shader flag reads 0"
    );
    assert!(
        linear_lighting_on(&none, false, true),
        "new LL keeps the P0 default"
    );
    let fd = feature_data(&none, &f, true, false);
    assert_eq!(
        value(&fd, "linearLightingSettings.enableLinearLighting"),
        [0.0]
    );
    assert_eq!(
        value(&fd, "linearLightingSettings.pointLightMult"),
        [1.0],
        "not the capture pi"
    );
}

/// Each build triggers exactly its own rule (fixtures: every pointLightMult key is 1).
#[test]
fn each_build_triggers_its_own_linear_lighting_rule() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let fd = feature_data(&s, &f, true, false);
        let point = value(&fd, "linearLightingSettings.pointLightMult")[0];
        if tag == "aio190" {
            assert_eq!(point, PI, "{tag}");
            assert!(!fd.iter().any(|(n, _)| n == "emissiveMult"), "{tag}");
        } else {
            assert_eq!(point, 1.0, "{tag}");
            assert_eq!(
                value(&fd, "linearLightingSettings.dirLightMult"),
                [1.0],
                "{tag}"
            );
            assert_eq!(value(&fd, "emissiveMult"), [1.0], "{tag}");
        }
    }
}

/// 1.6/1.8 write the skin keys flat; CS reads the section root when there is no DefaultProfile (advanced-skin
/// Skin.cpp:22). Old packing: x = tiling (the shader multiplies by y, 1.8 Lighting.hlsl:2049), w = detail && skin,
/// sssParams.z and physicalParams.w unused (0).
#[test]
fn flat_skin_settings_are_read_like_cs_does() {
    let s = with(serde_json::json!({"Advanced Skin": {
        "EnableSkin": true, "SkinMainRoughness": 0.5, "EnableSkinDetail": true, "SkinDetailTiling": 10.0,
        "BodyTilingMultiplier": 2.0, "SkinDetailStrength": 0.3, "Translucency": 0.25
    }}));
    let fd = feature_data(&s, &fields_of("cs18_bottle"), true, false);
    assert_eq!(value(&fd, "skinData.skinParams")[0], 0.5);
    assert_eq!(
        value(&fd, "skinData.skinDetailParams"),
        [10.0, 2.0, 0.3, 1.0]
    );
    assert_eq!(value(&fd, "skinData.sssParams")[0], 0.25);
    assert_eq!(value(&fd, "skinData.sssParams")[2], 0.0);
    assert_eq!(value(&fd, "skinData.physicalParams")[3], 0.0);
    let skin_off =
        with(serde_json::json!({"Advanced Skin": {"EnableSkin": false, "EnableSkinDetail": true}}));
    let fd = feature_data(&skin_off, &fields_of("cs18_bottle"), true, false);
    assert_eq!(
        value(&fd, "skinData.skinDetailParams")[3],
        0.0,
        "old w = EnableSkinDetail && EnableSkin"
    );
}

/// Golden, capture 11178 (owner settings, installed build): body b7 skinDetailParams (20, 2, 0.25, 0) at ev 4386,
/// head (FaceGen, ev 4182) (10, 1, 0.25, 0); FeatureData b6 stays the body value for both (GetCommonBufferData).
#[test]
fn new_packing_premultiplies_and_heads_use_tiling_one() {
    let owner = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cs_settings"
    ));
    let s = read_settings(&SettingsPaths {
        user: Some(owner.join("SettingsUser.json")),
        default: Some(owner.join("SettingsDefault.json")),
        ..Default::default()
    });
    let f = fields_of("aio190");
    let body = feature_data(&s, &f, true, false);
    let head = feature_data(&s, &f, true, true);
    assert_eq!(
        value(&body, "skinPerGeometryProfile.skinDetailParams"),
        [20.0, 2.0, 0.25, 0.0]
    );
    assert_eq!(
        value(&head, "skinPerGeometryProfile.skinDetailParams"),
        [10.0, 1.0, 0.25, 0.0]
    );
    assert_eq!(
        value(&head, "skinData.skinDetailParams"),
        [20.0, 2.0, 0.25, 0.0]
    );
}

/// Each build packs tiling its own way, also with no skin section at all (old shaders would double the capture's
/// pre-multiplied 20).
#[test]
fn each_build_packs_skin_tiling_its_own_way() {
    for tag in BUILDS {
        let f = fields_of(tag);
        for s in [
            CsSettings::default(),
            with(
                serde_json::json!({"Advanced Skin": {"SkinDetailTiling": 10.0, "BodyTilingMultiplier": 2.0}}),
            ),
        ] {
            let fd = feature_data(&s, &f, true, false);
            let want = if tag == "aio190" { 20.0 } else { 10.0 };
            assert_eq!(value(&fd, "skinData.skinDetailParams")[0], want, "{tag}");
        }
    }
}

/// Old packing, partial settings: a missing EnableSkin falls back to the capture value (skin on) before the AND.
#[test]
fn old_detail_flag_falls_back_per_flag() {
    let s = with(serde_json::json!({"Advanced Skin": {"EnableSkinDetail": true}}));
    let fd = feature_data(&s, &fields_of("cs18_bottle"), true, false);
    assert_eq!(value(&fd, "skinData.skinParams")[3], 1.0);
    assert_eq!(value(&fd, "skinData.skinDetailParams")[3], 1.0);
}

/// Non-default tiling inputs, so a hard-coded 20/2 or 10/1 cannot pass.
#[test]
fn skin_tiling_follows_the_settings_values() {
    let s = with(
        serde_json::json!({"Advanced Skin": {"SkinDetailTiling": 7.0, "BodyTilingMultiplier": 3.0}}),
    );
    let f = fields_of("aio190");
    let body = feature_data(&s, &f, true, false);
    let head = feature_data(&s, &f, true, true);
    assert_eq!(
        value(&body, "skinPerGeometryProfile.skinDetailParams")[..2],
        [21.0, 3.0]
    );
    assert_eq!(
        value(&head, "skinPerGeometryProfile.skinDetailParams")[..2],
        [7.0, 1.0]
    );
    let old = feature_data(&s, &fields_of("cs18_bottle"), true, false);
    assert_eq!(value(&old, "skinData.skinDetailParams")[..2], [7.0, 3.0]);
}

/// 03 report 6.3: with the rules of Tasks 2-3 every build lands its own keys; Nexus has no JSON, so the old-LL
/// gammas and the ExtMat/ExtTrans fields stay unset (zero), the honest answer.
#[test]
fn coverage_per_build() {
    for tag in BUILDS {
        let (f, s) = build(tag);
        let c = coverage(&s, &f, &feature_data(&s, &f, true, false));
        assert!(c.unmatched.is_empty(), "{tag}: {:?}", c.unmatched);
        if tag == "nexus191" {
            let want: BTreeSet<String> = [
                "extendedMaterialSettings.EnableComplexMaterial",
                "extendedMaterialSettings.EnableHeightBlending",
                "extendedMaterialSettings.EnableParallax",
                "extendedMaterialSettings.EnableParallaxWarpingFix",
                "extendedMaterialSettings.EnableShadows",
                "extendedMaterialSettings.EnableTerrainParallax",
                "extendedTranslucencySettings.MaterialModel",
                "extendedTranslucencySettings.Reduction",
                "extendedTranslucencySettings.Softness",
                "extendedTranslucencySettings.Strength",
                "linearLightingSettings.ambientGamma",
                "linearLightingSettings.colorGamma",
                "linearLightingSettings.effectAlphaGamma",
                "linearLightingSettings.effectGamma",
                "linearLightingSettings.emitColorGamma",
                "linearLightingSettings.emitColorMult",
                "linearLightingSettings.fogAlphaGamma",
                "linearLightingSettings.fogGamma",
                "linearLightingSettings.glowmapGamma",
                "linearLightingSettings.lightGamma",
                "linearLightingSettings.skyGamma",
                "linearLightingSettings.vanillaDiffuseColorMult",
                "linearLightingSettings.vlGamma",
                "linearLightingSettings.waterGamma",
            ]
            .map(String::from)
            .into();
            assert_eq!(c.unset, want);
        } else {
            assert_eq!(c, Coverage::default(), "{tag}");
            assert_eq!(c.line(), None);
        }
    }
}

/// Drift shows: an old-generation key on the new build is unmatched, a reflected field nothing writes is unset;
/// pads, struct-less sections and known JSON-only keys stay quiet.
#[test]
fn coverage_names_the_drift() {
    // The installed build own (clean) key set plus one drifted field and two drifted keys.
    let (mut f, mut s) = build("aio190");
    f.insert("linearLightingSettings.newThing".into());
    let section = |s: &CsSettings, name: &str| -> serde_json::Map<String, serde_json::Value> {
        s.merged[name].as_object().unwrap().clone()
    };
    let mut ll = section(&s, "Linear Lighting");
    ll.insert("lightGamma".into(), 2.2.into());
    s.merged.insert("Linear Lighting".into(), ll.into());
    let mut skin = section(&s, "Advanced Skin");
    skin["DefaultProfile"]
        .as_object_mut()
        .unwrap()
        .insert("NewSkinKnob".into(), 1.0.into());
    s.merged.insert("Advanced Skin".into(), skin.into());
    let c = coverage(&s, &f, &feature_data(&s, &f, true, false));
    assert_eq!(
        c.unset,
        BTreeSet::from(["linearLightingSettings.newThing".to_string()])
    );
    assert_eq!(
        c.unmatched,
        BTreeSet::from([
            "Advanced Skin.NewSkinKnob".to_string(),
            "Linear Lighting.lightGamma".to_string()
        ])
    );
    assert_eq!(
        c.line().as_deref(),
        Some("CS settings: 1 fields unset, 2 keys unmatched")
    );
}
