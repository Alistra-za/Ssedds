//! P1-D: shape-keyed CS settings rules and the coverage report over the four CS builds the owner has
//! (03-cs-settings-drift.md). Fixtures = name lists extracted by the P1-D plan Task 1 (no CS source, no CS values).
use render3d::cs_settings::{feature_data, read_settings, CsSettings, SettingsPaths};
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
