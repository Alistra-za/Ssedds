//! Cubemap sub-type and its `cubemap` profile section (cubemap support v1, DECISIONS Note DN).
use engine::config::*;
use engine::policy::allowed_encodings;
use engine::presets::PresetRegistry;

fn builtin_ids(reg: &PresetRegistry) -> Vec<String> {
    let mut ids: Vec<String> = reg.selectable().iter().map(|p| p.id.clone()).collect();
    ids.push("sdc-base".into());
    ids
}

#[test]
fn default_cubemap_section_is_not_serialised() {
    let reg = PresetRegistry::builtin().unwrap();
    for id in builtin_ids(&reg) {
        for t in [Target::Cs, Target::Vanilla] {
            for st in Subtype::ALL {
                let p = reg.profile(&id, t, st).unwrap();
                let v = serde_json::to_value(&p).unwrap();
                assert!(v.get("cubemap").is_none(), "{id}/{t:?}/{st:?}");
            }
        }
    }
}

#[test]
fn cubemap_profile_defaults() {
    let reg = PresetRegistry::builtin().unwrap();
    let p = reg
        .profile("cs-quality", Target::Cs, Subtype::Cubemap)
        .unwrap();
    assert_eq!(p.format, Format::Bc7);
    assert_eq!(
        (p.color.content, p.color.tag),
        (ColorContent::Srgb, ColorTag::Unorm)
    );
    assert_eq!(p.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(p.mips.levels, MipLevels::Keyword(MipKeyword::Full));
    assert_eq!(p.category, Category::Color);
}

#[test]
fn cubemap_encodings_are_unorm_bc1_bc3_bc7_rgba8() {
    for t in [Target::Cs, Target::Vanilla] {
        assert_eq!(
            allowed_encodings(Subtype::Cubemap, t),
            vec![
                (Format::Bc1, ColorTag::Unorm),
                (Format::Bc3, ColorTag::Unorm),
                (Format::Bc7, ColorTag::Unorm),
                (Format::Rgba8, ColorTag::Unorm),
            ],
            "{t:?}"
        );
    }
}
