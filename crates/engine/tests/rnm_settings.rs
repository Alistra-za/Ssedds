//! M6 Track F2 Task 1: the `rnm.*` settings (F2 spec §4, DECISIONS Note W3, W6, W7).
use engine::config::*;
use engine::overrides::{parse_set, FIELD_PATHS, RNM_TARGET_FIELD};
use engine::presets::PresetRegistry;
use serde_json::json;

fn normal() -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::Normal)
        .unwrap()
}

#[test]
fn default_rnm_is_skipped_so_profile_json_is_unchanged() {
    let p = normal();
    assert!(p.rnm.is_default());
    assert!(serde_json::to_value(&p).unwrap().get("rnm").is_none());
}

#[test]
fn layer_defaults_follow_the_spec() {
    let r = RnmSpec::default();
    assert!(r.enabled);
    assert_eq!(
        r.effective_order(),
        vec![
            LayerId::Bent,
            LayerId::Bump,
            LayerId::Bump2,
            LayerId::Detail,
            LayerId::Detail2
        ]
    );
    let s = |id| r.layer(id).strength;
    assert_eq!(
        (
            s(LayerId::Bump),
            s(LayerId::Bump2),
            s(LayerId::Detail),
            s(LayerId::Detail2),
            s(LayerId::Bent)
        ),
        (0.4, 0.4, 0.6, 0.3, 0.35)
    );
    assert!(r.layer(LayerId::Detail).mask && r.layer(LayerId::Detail).flip_green.is_none());
    assert_eq!(r.ao_power(), 1.5);
    assert_eq!(r.bent.mode, BentMode::AoBlend);
    assert!(r.bent.ao);
}

#[test]
fn partial_json_fills_the_rest_with_defaults() {
    let r: RnmSpec = serde_json::from_value(
        json!({"detail2": {"mask": false}, "order": ["detail", "bump", "detail"]}),
    )
    .unwrap();
    assert_eq!(r.layer(LayerId::Detail2).strength, 0.3);
    assert!(!r.layer(LayerId::Detail2).mask);
    // Duplicates drop, missing ids follow in the default order (Note W6).
    assert_eq!(
        r.effective_order(),
        vec![
            LayerId::Detail,
            LayerId::Bump,
            LayerId::Bent,
            LayerId::Bump2,
            LayerId::Detail2
        ]
    );
    assert!(serde_json::from_value::<RnmSpec>(json!({"bogus": 1})).is_err());
    assert!(serde_json::from_value::<RnmSpec>(json!({"order": ["nope"]})).is_err());
}

#[test]
fn field_paths_list_every_rnm_key() {
    let rnm: Vec<&&str> = FIELD_PATHS
        .iter()
        .filter(|f| f.starts_with("rnm."))
        .collect();
    assert_eq!(rnm.len(), 21);
    for l in ["bump", "bump2", "detail", "detail2", "bent"] {
        for leaf in ["strength", "flip_green", "mask"] {
            assert!(FIELD_PATHS.contains(&format!("rnm.{l}.{leaf}").as_str()));
        }
    }
    for f in [
        "rnm.enabled",
        "rnm.order",
        "rnm.bent.mode",
        "rnm.bent.ao_power",
        "rnm.bent.ao",
        RNM_TARGET_FIELD,
    ] {
        assert!(FIELD_PATHS.contains(&f), "{f}");
    }
}

#[test]
fn rnm_target_is_a_per_file_selector() {
    assert!(parse_set("file:*_bump.png.rnm.target=both").is_ok());
    assert!(parse_set("rnm.target=cnr").is_ok());
    assert!(parse_set("rnm.target=sideways").is_err());
    assert!(parse_set("subtype:normal.rnm.target=n").is_err());
    assert!(parse_set("subtype:normal.rnm.detail.strength=0.8").is_ok());
}

#[test]
fn rnm_base_subtypes() {
    assert_eq!(Subtype::Normal.rnm_base(), Some(RnmTarget::N));
    assert_eq!(Subtype::PbrNormal.rnm_base(), Some(RnmTarget::N));
    assert_eq!(Subtype::LandscapeNormal.rnm_base(), Some(RnmTarget::N));
    assert_eq!(Subtype::CoatNormal.rnm_base(), Some(RnmTarget::Cnr));
    assert_eq!(Subtype::Msn.rnm_base(), None);
    assert_eq!(Subtype::Diffuse.rnm_base(), None);
}
